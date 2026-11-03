//! Muzzle flashes: the dynamic light and the gunfire sound a weapon
//! makes. C ref: `CL_AddMuzzleFlash`/`CL_AddMuzzleFlash2`
//! (client/cl_effects.c) and the dlight list of client/cl_lights.c.
//! Their particles are queued for `fx::ClientFx::flash_particles`.

use q2_common::pmove::angle_vectors;
use q2_shared::constants::*;
use q2_shared::flash::MONSTER_FLASH_OFFSET;
use q2_shared::types::{EntityState, Vec3f};

/// C `MAX_DLIGHTS`.
pub const MAX_DLIGHTS: usize = 32;

use crate::sound::NamedSound;

/// A client dynamic light (C `cdlight_t`). `die` is in client
/// milliseconds; a light shows until 32 ms after it (yquake2).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClientDLight {
    /// The entity it belongs to; a new flash from it reuses the light.
    pub key: i32,
    pub color: [f32; 3],
    pub origin: Vec3f,
    pub radius: f32,
    pub die: f32,
    /// Radius lost per second.
    pub decay: f32,
    pub minlight: f32,
}

/// The client's dynamic lights (C `cl_dlights`).
#[derive(Debug, Clone)]
pub struct Effects {
    dlights: [ClientDLight; MAX_DLIGHTS],
    /// Flashes' particles, for the particle effects to take.
    flash_particles: Vec<(Vec3f, FlashParticles)>,
}

/// The particles a flash makes: a monster's at its muzzle (C
/// `CL_AddMuzzleFlash2`'s `CL_ParticleEffect` and `CL_SmokeAndFlash`), a
/// player's login at the player (C `CL_AddMuzzleFlash`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashParticles {
    Smoke,
    /// 40 sparks of palette colour 0, then the smoke.
    SparksAndSmoke,
    /// `SparksAndSmoke`, then entity n's `tank/tnkatk2a.wav` .. `e`,
    /// whose letter C draws from randk after the sparks.
    TankMachinegun(i32),
    /// C `CL_LogoutEffect` for this `MZ_LOGIN`, `MZ_LOGOUT` or
    /// `MZ_RESPAWN` flash.
    Logout(i32),
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            dlights: [ClientDLight::default(); MAX_DLIGHTS],
            flash_particles: Vec::new(),
        }
    }
}

impl Effects {
    /// C `CL_ClearDlights` (on a map change).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The light for `key`: its own, else one that died, else the first.
    /// C ref: `CL_AllocDlight`.
    fn alloc_dlight(&mut self, key: i32, time: f32) -> &mut ClientDLight {
        let index = (key != 0)
            .then(|| self.dlights.iter().position(|d| d.key == key))
            .flatten()
            .or_else(|| self.dlights.iter().position(|d| d.die < time))
            .unwrap_or(0);
        let light = &mut self.dlights[index];
        light.key = key;
        light
    }

    /// Light `light` in its key's slot, else one that died, else the
    /// first, as the effects' `CL_AllocDlight` calls do.
    pub fn add_dlight(&mut self, light: ClientDLight, time: f32) {
        *self.alloc_dlight(light.key, time) = light;
    }

    /// Shrink lights by their decay over `frametime` seconds and put out
    /// the ones past their time. C ref: `CL_RunDLights`.
    pub fn run_dlights(&mut self, time: f32, frametime: f32) {
        for light in self.dlights.iter_mut().filter(|d| d.radius != 0.0) {
            if light.die < time - 32.0 {
                light.radius = 0.0;
                continue;
            }
            light.radius = (light.radius - frametime * light.decay).max(0.0);
        }
    }

    /// The lit lights this frame (C `CL_AddDLights`).
    pub fn dlights(&self) -> impl Iterator<Item = &ClientDLight> {
        self.dlights.iter().filter(|d| d.radius != 0.0)
    }

    /// A player's weapon fired: a light ahead and right of the player and
    /// the weapon's sound. `weapon` carries `MZ_SILENCED`. Covers the
    /// baseq2, xatrix and rogue flashes; any other number keeps the
    /// light's old colour and plays nothing, as C's switch has no default.
    /// Returns the sounds to start. C ref: `CL_AddMuzzleFlash`.
    pub fn muzzle_flash(
        &mut self,
        player: &EntityState,
        weapon: i32,
        time: f32,
        randk: &mut impl FnMut() -> u32,
    ) -> Vec<NamedSound> {
        let entnum = player.number;
        let silenced = weapon & MZ_SILENCED != 0;
        let weapon = weapon & !MZ_SILENCED;
        let (forward, right, _) = angle_vectors(player.angles);
        let radius = if silenced { 100.0 } else { 200.0 } + (randk() & 31) as f32;
        let volume = if silenced { 0.2 } else { 1.0 };
        let light = self.alloc_dlight(entnum, time);
        light.origin = player.origin + forward * 18.0 + right * 16.0;
        light.radius = radius;
        light.minlight = 32.0;
        light.die = time;

        let mut sounds = Vec::new();
        let mut sound = |channel, name: String, timeofs| {
            sounds.push(NamedSound {
                name,
                entnum,
                entchannel: channel,
                volume,
                attenuation: ATTN_NORM,
                timeofs,
                origin: None,
            });
        };
        let machinegun =
            |randk: &mut dyn FnMut() -> u32| format!("weapons/machgf{}b.wav", randk() % 5 + 1);
        let color = match weapon {
            MZ_BLASTER => {
                sound(CHAN_WEAPON, "weapons/blastf1a.wav".into(), 0.0);
                [1.0, 1.0, 0.0]
            }
            MZ_HYPERBLASTER => {
                sound(CHAN_WEAPON, "weapons/hyprbf1a.wav".into(), 0.0);
                [1.0, 1.0, 0.0]
            }
            MZ_MACHINEGUN => {
                sound(CHAN_WEAPON, machinegun(randk), 0.0);
                [1.0, 1.0, 0.0]
            }
            MZ_SHOTGUN => {
                sound(CHAN_WEAPON, "weapons/shotgf1b.wav".into(), 0.0);
                sound(CHAN_AUTO, "weapons/shotgr1b.wav".into(), 0.1);
                [1.0, 1.0, 0.0]
            }
            MZ_SSHOTGUN => {
                sound(CHAN_WEAPON, "weapons/sshotf1b.wav".into(), 0.0);
                [1.0, 1.0, 0.0]
            }
            MZ_CHAINGUN1 | MZ_CHAINGUN2 | MZ_CHAINGUN3 => {
                let spin = weapon - MZ_CHAINGUN1;
                let base = [200.0, 225.0, 250.0][spin as usize];
                let light = self.alloc_dlight(entnum, time);
                light.radius = base + (randk() & 31) as f32;
                if spin > 0 {
                    light.die = time + 0.1; // C: "long delay" of 0.1 ms
                }
                for (shot, timeofs) in [0.0, 0.05, 0.033, 0.066].into_iter().enumerate() {
                    let wanted = match spin {
                        0 => shot == 0,
                        1 => shot < 2,
                        _ => shot != 1,
                    };
                    if wanted {
                        sound(CHAN_WEAPON, machinegun(randk), timeofs);
                    }
                }
                [[1.0, 0.25, 0.0], [1.0, 0.5, 0.0], [1.0, 1.0, 0.0]][spin as usize]
            }
            MZ_RAILGUN => {
                sound(CHAN_WEAPON, "weapons/railgf1a.wav".into(), 0.0);
                [0.5, 0.5, 1.0]
            }
            MZ_ROCKET => {
                sound(CHAN_WEAPON, "weapons/rocklf1a.wav".into(), 0.0);
                sound(CHAN_AUTO, "weapons/rocklr1b.wav".into(), 0.1);
                [1.0, 0.5, 0.2]
            }
            MZ_GRENADE => {
                sound(CHAN_WEAPON, "weapons/grenlf1a.wav".into(), 0.0);
                sound(CHAN_AUTO, "weapons/grenlr1b.wav".into(), 0.1);
                [1.0, 0.5, 0.0]
            }
            MZ_BFG => {
                sound(CHAN_WEAPON, "weapons/bfg__f1y.wav".into(), 0.0);
                [0.0, 1.0, 0.0]
            }
            MZ_LOGIN | MZ_LOGOUT | MZ_RESPAWN => {
                self.alloc_dlight(entnum, time).die = time + 1.0;
                self.flash_particles
                    .push((player.origin, FlashParticles::Logout(weapon)));
                sound(CHAN_WEAPON, "weapons/grenlf1a.wav".into(), 0.0);
                // Full volume even when silenced.
                if let Some(last) = sounds.last_mut() {
                    last.volume = 1.0;
                }
                match weapon {
                    MZ_LOGIN => [0.0, 1.0, 0.0],
                    MZ_LOGOUT => [1.0, 0.0, 0.0],
                    _ => [1.0, 1.0, 0.0],
                }
            }
            MZ_BLUEHYPERBLASTER => {
                sound(CHAN_WEAPON, "weapons/hyprbf1a.wav".into(), 0.0);
                [0.0, 0.0, 1.0]
            }
            MZ_PHALANX => {
                sound(CHAN_WEAPON, "weapons/plasshot.wav".into(), 0.0);
                [1.0, 0.5, 0.5]
            }
            MZ_IONRIPPER => {
                sound(CHAN_WEAPON, "weapons/rippfire.wav".into(), 0.0);
                [1.0, 0.5, 0.5]
            }
            MZ_ETF_RIFLE => {
                sound(CHAN_WEAPON, "weapons/nail1.wav".into(), 0.0);
                [0.9, 0.7, 0.0]
            }
            MZ_SHOTGUN2 => {
                sound(CHAN_WEAPON, "weapons/shotg2.wav".into(), 0.0);
                [1.0, 1.0, 0.0]
            }
            MZ_BLASTER2 => {
                sound(CHAN_WEAPON, "weapons/blastf1a.wav".into(), 0.0);
                [0.0, 1.0, 0.0]
            }
            MZ_TRACKER => {
                // C: "negative flashes handled the same in gl/soft until
                // CL_AddDLights".
                sound(CHAN_WEAPON, "weapons/disint2.wav".into(), 0.0);
                [-1.0, -1.0, -1.0]
            }
            MZ_HEATBEAM | MZ_NUKE1 | MZ_NUKE2 | MZ_NUKE4 | MZ_NUKE8 => {
                self.alloc_dlight(entnum, time).die = time + 100.0;
                match weapon {
                    MZ_NUKE1 => [1.0, 0.0, 0.0],
                    MZ_NUKE4 => [0.0, 0.0, 1.0],
                    MZ_NUKE8 => [0.0, 1.0, 1.0],
                    _ => [1.0, 1.0, 0.0],
                }
            }
            _ => self.alloc_dlight(entnum, time).color,
        };
        self.alloc_dlight(entnum, time).color = color;
        sounds
    }

    /// A monster's weapon fired: a light at its `MZ2_*` muzzle and the
    /// monster's attack sound. Returns the sounds to start; none for a
    /// flash number past the table. C ref: `CL_AddMuzzleFlash2`.
    pub fn muzzle_flash2(
        &mut self,
        monster: &EntityState,
        flash: i32,
        time: f32,
        randk: &mut impl FnMut() -> u32,
    ) -> Vec<NamedSound> {
        let Some(offset) = usize::try_from(flash)
            .ok()
            .filter(|&f| f <= 210)
            .map(|f| MONSTER_FLASH_OFFSET[f])
        else {
            tracing::debug!("CL_AddMuzzleFlash2: bad offset {flash}");
            return Vec::new();
        };
        let (forward, right, _) = angle_vectors(monster.angles);
        let mut origin = monster.origin + forward * offset[0] + right * offset[1];
        origin.z += offset[2];
        let entnum = monster.number;
        let radius = 200.0 + (randk() & 31) as f32;
        let light = self.alloc_dlight(entnum, time);
        light.origin = origin;
        light.radius = radius;
        light.minlight = 32.0;
        light.die = time;
        if let Some(smoke) = monster_flash_smoke(flash, entnum) {
            self.flash_particles.push((origin, smoke));
        }
        let Some(def) = monster_flash(flash) else {
            return Vec::new();
        };
        let light = self.alloc_dlight(entnum, time);
        light.color = def.color;
        if def.plasma {
            light.radius = 300.0 + (randk() & 100) as f32;
            light.die = time + 200.0;
        }
        let (name, attenuation) = match def.sound {
            FlashSoundName::None => return Vec::new(),
            FlashSoundName::Named(name, attenuation) => (name.to_string(), attenuation),
            // Started by `FlashParticles::TankMachinegun`, after the sparks.
            FlashSoundName::TankMachinegun => return Vec::new(),
        };
        vec![NamedSound::on(entnum, CHAN_WEAPON, &name, attenuation)]
    }
}

impl Effects {
    /// The flash particles queued since the last call.
    pub fn take_particles(&mut self) -> Vec<(Vec3f, FlashParticles)> {
        std::mem::take(&mut self.flash_particles)
    }

    /// Run one server message for its effects: a player or monster muzzle
    /// flash against the entity it names (`entity` looks up its current
    /// state, C `cl_entities[n].current`). Other messages, and flashes of
    /// entities the client does not have, do nothing; a short body or an
    /// entity outside 1..MAX_EDICTS-1 is logged and does nothing (C drops
    /// the connection). Returns the sounds
    /// to start. C ref: `CL_ParseServerMessage`'s `svc_muzzleflash` and
    /// `svc_muzzleflash2` cases.
    pub fn message(
        &mut self,
        bytes: &[u8],
        entity: impl Fn(i32) -> Option<EntityState>,
        time: f32,
        randk: &mut impl FnMut() -> u32,
    ) -> Vec<NamedSound> {
        use q2_shared::protocol::SvcOp;
        let mut msg = q2_common::net_msg::NetMsg::from_bytes(bytes);
        msg.begin_reading();
        let op = SvcOp::try_from(msg.read_byte() as u8);
        if !matches!(op, Ok(SvcOp::MuzzleFlash) | Ok(SvcOp::MuzzleFlash2)) {
            return Vec::new();
        }
        let number = msg.read_short();
        let flash = msg.read_byte();
        // C acts on a short body and drops at the next op ("Bad server
        // message"); a cut flash byte reads -1, so this drops it first.
        if msg.is_overflowed() {
            tracing::warn!("bad {op:?}: message ends early");
            return Vec::new();
        }
        // C ref: Com_Error(ERR_DROP, "CL_AddMuzzleFlash{,2}: bad entity").
        if !(1..MAX_EDICTS as i32).contains(&number) {
            tracing::warn!("bad {op:?}: bad entity {number}");
            return Vec::new();
        }
        let Some(state) = entity(number) else {
            return Vec::new();
        };
        if op == Ok(SvcOp::MuzzleFlash) {
            self.muzzle_flash(&state, flash, time, randk)
        } else {
            self.muzzle_flash2(&state, flash, time, randk)
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FlashSoundName {
    None,
    Named(&'static str, f32),
    /// `tank/tnkatk2a.wav` .. `e`, at random, picked by
    /// `FlashParticles::TankMachinegun`.
    TankMachinegun,
}

#[derive(Debug, Clone, Copy)]
struct MonsterFlash {
    color: [f32; 3],
    sound: FlashSoundName,
    /// The widow's plasma beam: a bigger light that lasts 200 ms.
    plasma: bool,
}

/// C `CL_AddMuzzleFlash2`'s switch, as data.
/// The base game's machineguns spark and smoke; the soldier's shotgun
/// only smokes. `entnum` is the monster, for the tank's sound.
fn monster_flash_smoke(flash: i32, entnum: i32) -> Option<FlashParticles> {
    match flash {
        MZ2_INFANTRY_MACHINEGUN_1
        | MZ2_INFANTRY_MACHINEGUN_2
        | MZ2_INFANTRY_MACHINEGUN_3
        | MZ2_INFANTRY_MACHINEGUN_4
        | MZ2_INFANTRY_MACHINEGUN_5
        | MZ2_INFANTRY_MACHINEGUN_6
        | MZ2_INFANTRY_MACHINEGUN_7
        | MZ2_INFANTRY_MACHINEGUN_8
        | MZ2_INFANTRY_MACHINEGUN_9
        | MZ2_INFANTRY_MACHINEGUN_10
        | MZ2_INFANTRY_MACHINEGUN_11
        | MZ2_INFANTRY_MACHINEGUN_12
        | MZ2_INFANTRY_MACHINEGUN_13
        | MZ2_SOLDIER_MACHINEGUN_1
        | MZ2_SOLDIER_MACHINEGUN_2
        | MZ2_SOLDIER_MACHINEGUN_3
        | MZ2_SOLDIER_MACHINEGUN_4
        | MZ2_SOLDIER_MACHINEGUN_5
        | MZ2_SOLDIER_MACHINEGUN_6
        | MZ2_SOLDIER_MACHINEGUN_7
        | MZ2_SOLDIER_MACHINEGUN_8
        | MZ2_GUNNER_MACHINEGUN_1
        | MZ2_GUNNER_MACHINEGUN_2
        | MZ2_GUNNER_MACHINEGUN_3
        | MZ2_GUNNER_MACHINEGUN_4
        | MZ2_GUNNER_MACHINEGUN_5
        | MZ2_GUNNER_MACHINEGUN_6
        | MZ2_GUNNER_MACHINEGUN_7
        | MZ2_GUNNER_MACHINEGUN_8
        | MZ2_ACTOR_MACHINEGUN_1
        | MZ2_SUPERTANK_MACHINEGUN_1
        | MZ2_SUPERTANK_MACHINEGUN_2
        | MZ2_SUPERTANK_MACHINEGUN_3
        | MZ2_SUPERTANK_MACHINEGUN_4
        | MZ2_SUPERTANK_MACHINEGUN_5
        | MZ2_SUPERTANK_MACHINEGUN_6
        | MZ2_TURRET_MACHINEGUN
        | MZ2_BOSS2_MACHINEGUN_L1
        | MZ2_BOSS2_MACHINEGUN_L2
        | MZ2_BOSS2_MACHINEGUN_L3
        | MZ2_BOSS2_MACHINEGUN_L4
        | MZ2_BOSS2_MACHINEGUN_L5
        | MZ2_CARRIER_MACHINEGUN_L1
        | MZ2_CARRIER_MACHINEGUN_L2
        | MZ2_JORG_MACHINEGUN_L1
        | MZ2_JORG_MACHINEGUN_L2
        | MZ2_JORG_MACHINEGUN_L3
        | MZ2_JORG_MACHINEGUN_L4
        | MZ2_JORG_MACHINEGUN_L5
        | MZ2_JORG_MACHINEGUN_L6
        | MZ2_JORG_MACHINEGUN_R1
        | MZ2_JORG_MACHINEGUN_R2
        | MZ2_JORG_MACHINEGUN_R3
        | MZ2_JORG_MACHINEGUN_R4
        | MZ2_JORG_MACHINEGUN_R5
        | MZ2_JORG_MACHINEGUN_R6
        | MZ2_BOSS2_MACHINEGUN_R1
        | MZ2_BOSS2_MACHINEGUN_R2
        | MZ2_BOSS2_MACHINEGUN_R3
        | MZ2_BOSS2_MACHINEGUN_R4
        | MZ2_BOSS2_MACHINEGUN_R5
        | MZ2_CARRIER_MACHINEGUN_R1
        | MZ2_CARRIER_MACHINEGUN_R2 => Some(FlashParticles::SparksAndSmoke),
        MZ2_TANK_MACHINEGUN_1
        | MZ2_TANK_MACHINEGUN_2
        | MZ2_TANK_MACHINEGUN_3
        | MZ2_TANK_MACHINEGUN_4
        | MZ2_TANK_MACHINEGUN_5
        | MZ2_TANK_MACHINEGUN_6
        | MZ2_TANK_MACHINEGUN_7
        | MZ2_TANK_MACHINEGUN_8
        | MZ2_TANK_MACHINEGUN_9
        | MZ2_TANK_MACHINEGUN_10
        | MZ2_TANK_MACHINEGUN_11
        | MZ2_TANK_MACHINEGUN_12
        | MZ2_TANK_MACHINEGUN_13
        | MZ2_TANK_MACHINEGUN_14
        | MZ2_TANK_MACHINEGUN_15
        | MZ2_TANK_MACHINEGUN_16
        | MZ2_TANK_MACHINEGUN_17
        | MZ2_TANK_MACHINEGUN_18
        | MZ2_TANK_MACHINEGUN_19 => Some(FlashParticles::TankMachinegun(entnum)),
        MZ2_SOLDIER_SHOTGUN_1
        | MZ2_SOLDIER_SHOTGUN_2
        | MZ2_SOLDIER_SHOTGUN_3
        | MZ2_SOLDIER_SHOTGUN_4
        | MZ2_SOLDIER_SHOTGUN_5
        | MZ2_SOLDIER_SHOTGUN_6
        | MZ2_SOLDIER_SHOTGUN_7
        | MZ2_SOLDIER_SHOTGUN_8 => Some(FlashParticles::Smoke),
        _ => None,
    }
}

fn monster_flash(flash: i32) -> Option<MonsterFlash> {
    use FlashSoundName as FlashSound;
    match flash {
        MZ2_INFANTRY_MACHINEGUN_1
        | MZ2_INFANTRY_MACHINEGUN_2
        | MZ2_INFANTRY_MACHINEGUN_3
        | MZ2_INFANTRY_MACHINEGUN_4
        | MZ2_INFANTRY_MACHINEGUN_5
        | MZ2_INFANTRY_MACHINEGUN_6
        | MZ2_INFANTRY_MACHINEGUN_7
        | MZ2_INFANTRY_MACHINEGUN_8
        | MZ2_INFANTRY_MACHINEGUN_9
        | MZ2_INFANTRY_MACHINEGUN_10
        | MZ2_INFANTRY_MACHINEGUN_11
        | MZ2_INFANTRY_MACHINEGUN_12
        | MZ2_INFANTRY_MACHINEGUN_13 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("infantry/infatck1.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_SOLDIER_MACHINEGUN_1
        | MZ2_SOLDIER_MACHINEGUN_2
        | MZ2_SOLDIER_MACHINEGUN_3
        | MZ2_SOLDIER_MACHINEGUN_4
        | MZ2_SOLDIER_MACHINEGUN_5
        | MZ2_SOLDIER_MACHINEGUN_6
        | MZ2_SOLDIER_MACHINEGUN_7
        | MZ2_SOLDIER_MACHINEGUN_8 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("soldier/solatck3.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_GUNNER_MACHINEGUN_1
        | MZ2_GUNNER_MACHINEGUN_2
        | MZ2_GUNNER_MACHINEGUN_3
        | MZ2_GUNNER_MACHINEGUN_4
        | MZ2_GUNNER_MACHINEGUN_5
        | MZ2_GUNNER_MACHINEGUN_6
        | MZ2_GUNNER_MACHINEGUN_7
        | MZ2_GUNNER_MACHINEGUN_8 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("gunner/gunatck2.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_ACTOR_MACHINEGUN_1
        | MZ2_SUPERTANK_MACHINEGUN_1
        | MZ2_SUPERTANK_MACHINEGUN_2
        | MZ2_SUPERTANK_MACHINEGUN_3
        | MZ2_SUPERTANK_MACHINEGUN_4
        | MZ2_SUPERTANK_MACHINEGUN_5
        | MZ2_SUPERTANK_MACHINEGUN_6
        | MZ2_TURRET_MACHINEGUN => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("infantry/infatck1.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_BOSS2_MACHINEGUN_L1
        | MZ2_BOSS2_MACHINEGUN_L2
        | MZ2_BOSS2_MACHINEGUN_L3
        | MZ2_BOSS2_MACHINEGUN_L4
        | MZ2_BOSS2_MACHINEGUN_L5
        | MZ2_CARRIER_MACHINEGUN_L1
        | MZ2_CARRIER_MACHINEGUN_L2 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("infantry/infatck1.wav", ATTN_NONE),
            plasma: false,
        }),
        MZ2_SOLDIER_BLASTER_1
        | MZ2_SOLDIER_BLASTER_2
        | MZ2_SOLDIER_BLASTER_3
        | MZ2_SOLDIER_BLASTER_4
        | MZ2_SOLDIER_BLASTER_5
        | MZ2_SOLDIER_BLASTER_6
        | MZ2_SOLDIER_BLASTER_7
        | MZ2_SOLDIER_BLASTER_8
        | MZ2_TURRET_BLASTER => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("soldier/solatck2.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_FLYER_BLASTER_1 | MZ2_FLYER_BLASTER_2 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("flyer/flyatck3.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_MEDIC_BLASTER_1 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("medic/medatck1.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_HOVER_BLASTER_1 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("hover/hovatck1.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_FLOAT_BLASTER_1 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("floater/fltatck1.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_SOLDIER_SHOTGUN_1
        | MZ2_SOLDIER_SHOTGUN_2
        | MZ2_SOLDIER_SHOTGUN_3
        | MZ2_SOLDIER_SHOTGUN_4
        | MZ2_SOLDIER_SHOTGUN_5
        | MZ2_SOLDIER_SHOTGUN_6
        | MZ2_SOLDIER_SHOTGUN_7
        | MZ2_SOLDIER_SHOTGUN_8 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("soldier/solatck1.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_TANK_BLASTER_1 | MZ2_TANK_BLASTER_2 | MZ2_TANK_BLASTER_3 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("tank/tnkatck3.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_TANK_MACHINEGUN_1
        | MZ2_TANK_MACHINEGUN_2
        | MZ2_TANK_MACHINEGUN_3
        | MZ2_TANK_MACHINEGUN_4
        | MZ2_TANK_MACHINEGUN_5
        | MZ2_TANK_MACHINEGUN_6
        | MZ2_TANK_MACHINEGUN_7
        | MZ2_TANK_MACHINEGUN_8
        | MZ2_TANK_MACHINEGUN_9
        | MZ2_TANK_MACHINEGUN_10
        | MZ2_TANK_MACHINEGUN_11
        | MZ2_TANK_MACHINEGUN_12
        | MZ2_TANK_MACHINEGUN_13
        | MZ2_TANK_MACHINEGUN_14
        | MZ2_TANK_MACHINEGUN_15
        | MZ2_TANK_MACHINEGUN_16
        | MZ2_TANK_MACHINEGUN_17
        | MZ2_TANK_MACHINEGUN_18
        | MZ2_TANK_MACHINEGUN_19 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::TankMachinegun,
            plasma: false,
        }),
        MZ2_CHICK_ROCKET_1 | MZ2_TURRET_ROCKET => Some(MonsterFlash {
            color: [1.0, 0.5, 0.2],
            sound: FlashSound::Named("chick/chkatck2.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_TANK_ROCKET_1 | MZ2_TANK_ROCKET_2 | MZ2_TANK_ROCKET_3 => Some(MonsterFlash {
            color: [1.0, 0.5, 0.2],
            sound: FlashSound::Named("tank/tnkatck1.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_SUPERTANK_ROCKET_1
        | MZ2_SUPERTANK_ROCKET_2
        | MZ2_SUPERTANK_ROCKET_3
        | MZ2_BOSS2_ROCKET_1
        | MZ2_BOSS2_ROCKET_2
        | MZ2_BOSS2_ROCKET_3
        | MZ2_BOSS2_ROCKET_4
        | MZ2_CARRIER_ROCKET_1 => Some(MonsterFlash {
            color: [1.0, 0.5, 0.2],
            sound: FlashSound::Named("tank/rocket.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_GUNNER_GRENADE_1 | MZ2_GUNNER_GRENADE_2 | MZ2_GUNNER_GRENADE_3
        | MZ2_GUNNER_GRENADE_4 => Some(MonsterFlash {
            color: [1.0, 0.5, 0.0],
            sound: FlashSound::Named("gunner/gunatck3.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_GLADIATOR_RAILGUN_1 | MZ2_CARRIER_RAILGUN | MZ2_WIDOW_RAIL => Some(MonsterFlash {
            color: [0.5, 0.5, 1.0],
            sound: FlashSound::None,
            plasma: false,
        }),
        MZ2_MAKRON_BFG => Some(MonsterFlash {
            color: [0.5, 1.0, 0.5],
            sound: FlashSound::None,
            plasma: false,
        }),
        MZ2_MAKRON_BLASTER_1
        | MZ2_MAKRON_BLASTER_2
        | MZ2_MAKRON_BLASTER_3
        | MZ2_MAKRON_BLASTER_4
        | MZ2_MAKRON_BLASTER_5
        | MZ2_MAKRON_BLASTER_6
        | MZ2_MAKRON_BLASTER_7
        | MZ2_MAKRON_BLASTER_8
        | MZ2_MAKRON_BLASTER_9
        | MZ2_MAKRON_BLASTER_10
        | MZ2_MAKRON_BLASTER_11
        | MZ2_MAKRON_BLASTER_12
        | MZ2_MAKRON_BLASTER_13
        | MZ2_MAKRON_BLASTER_14
        | MZ2_MAKRON_BLASTER_15
        | MZ2_MAKRON_BLASTER_16
        | MZ2_MAKRON_BLASTER_17 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("makron/blaster.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_JORG_MACHINEGUN_L1
        | MZ2_JORG_MACHINEGUN_L2
        | MZ2_JORG_MACHINEGUN_L3
        | MZ2_JORG_MACHINEGUN_L4
        | MZ2_JORG_MACHINEGUN_L5
        | MZ2_JORG_MACHINEGUN_L6 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::Named("boss3/xfire.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_JORG_MACHINEGUN_R1
        | MZ2_JORG_MACHINEGUN_R2
        | MZ2_JORG_MACHINEGUN_R3
        | MZ2_JORG_MACHINEGUN_R4
        | MZ2_JORG_MACHINEGUN_R5
        | MZ2_JORG_MACHINEGUN_R6 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::None,
            plasma: false,
        }),
        MZ2_JORG_BFG_1 => Some(MonsterFlash {
            color: [0.5, 1.0, 0.5],
            sound: FlashSound::None,
            plasma: false,
        }),
        MZ2_BOSS2_MACHINEGUN_R1
        | MZ2_BOSS2_MACHINEGUN_R2
        | MZ2_BOSS2_MACHINEGUN_R3
        | MZ2_BOSS2_MACHINEGUN_R4
        | MZ2_BOSS2_MACHINEGUN_R5
        | MZ2_CARRIER_MACHINEGUN_R1
        | MZ2_CARRIER_MACHINEGUN_R2 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::None,
            plasma: false,
        }),
        MZ2_STALKER_BLASTER
        | MZ2_DAEDALUS_BLASTER
        | MZ2_MEDIC_BLASTER_2
        | MZ2_WIDOW_BLASTER
        | MZ2_WIDOW_BLASTER_SWEEP1
        | MZ2_WIDOW_BLASTER_SWEEP2
        | MZ2_WIDOW_BLASTER_SWEEP3
        | MZ2_WIDOW_BLASTER_SWEEP4
        | MZ2_WIDOW_BLASTER_SWEEP5
        | MZ2_WIDOW_BLASTER_SWEEP6
        | MZ2_WIDOW_BLASTER_SWEEP7
        | MZ2_WIDOW_BLASTER_SWEEP8
        | MZ2_WIDOW_BLASTER_SWEEP9
        | MZ2_WIDOW_BLASTER_100
        | MZ2_WIDOW_BLASTER_90
        | MZ2_WIDOW_BLASTER_80
        | MZ2_WIDOW_BLASTER_70
        | MZ2_WIDOW_BLASTER_60
        | MZ2_WIDOW_BLASTER_50
        | MZ2_WIDOW_BLASTER_40
        | MZ2_WIDOW_BLASTER_30
        | MZ2_WIDOW_BLASTER_20
        | MZ2_WIDOW_BLASTER_10
        | MZ2_WIDOW_BLASTER_0
        | MZ2_WIDOW_BLASTER_10L
        | MZ2_WIDOW_BLASTER_20L
        | MZ2_WIDOW_BLASTER_30L
        | MZ2_WIDOW_BLASTER_40L
        | MZ2_WIDOW_BLASTER_50L
        | MZ2_WIDOW_BLASTER_60L
        | MZ2_WIDOW_BLASTER_70L
        | MZ2_WIDOW_RUN_1
        | MZ2_WIDOW_RUN_2
        | MZ2_WIDOW_RUN_3
        | MZ2_WIDOW_RUN_4
        | MZ2_WIDOW_RUN_5
        | MZ2_WIDOW_RUN_6
        | MZ2_WIDOW_RUN_7
        | MZ2_WIDOW_RUN_8 => Some(MonsterFlash {
            color: [0.0, 1.0, 0.0],
            sound: FlashSound::Named("tank/tnkatck3.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_WIDOW_DISRUPTOR => Some(MonsterFlash {
            color: [-1.0, -1.0, -1.0],
            sound: FlashSound::Named("weapons/disint2.wav", ATTN_NORM),
            plasma: false,
        }),
        MZ2_WIDOW_PLASMABEAM
        | MZ2_WIDOW2_BEAMER_1
        | MZ2_WIDOW2_BEAMER_2
        | MZ2_WIDOW2_BEAMER_3
        | MZ2_WIDOW2_BEAMER_4
        | MZ2_WIDOW2_BEAMER_5
        | MZ2_WIDOW2_BEAM_SWEEP_1
        | MZ2_WIDOW2_BEAM_SWEEP_2
        | MZ2_WIDOW2_BEAM_SWEEP_3
        | MZ2_WIDOW2_BEAM_SWEEP_4
        | MZ2_WIDOW2_BEAM_SWEEP_5
        | MZ2_WIDOW2_BEAM_SWEEP_6
        | MZ2_WIDOW2_BEAM_SWEEP_7
        | MZ2_WIDOW2_BEAM_SWEEP_8
        | MZ2_WIDOW2_BEAM_SWEEP_9
        | MZ2_WIDOW2_BEAM_SWEEP_10
        | MZ2_WIDOW2_BEAM_SWEEP_11 => Some(MonsterFlash {
            color: [1.0, 1.0, 0.0],
            sound: FlashSound::None,
            plasma: true,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_at(number: i32, yaw: f32) -> EntityState {
        EntityState {
            number,
            origin: Vec3f::new(100.0, 0.0, 24.0),
            angles: Vec3f::new(0.0, yaw, 0.0),
            ..Default::default()
        }
    }

    /// A fixed randk() sequence.
    fn rng(values: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    fn names(sounds: &[NamedSound]) -> Vec<(&str, i32, f32)> {
        sounds
            .iter()
            .map(|s| (s.name.as_str(), s.entchannel, s.timeofs))
            .collect()
    }

    #[test]
    fn a_blaster_flash_lights_ahead_and_right_of_the_player_and_sounds() {
        let mut fx = Effects::default();
        let sounds = fx.muzzle_flash(&player_at(1, 0.0), MZ_BLASTER, 500.0, &mut rng(&[5]));
        let light = *fx.dlights().next().unwrap();
        // Facing +x: forward is +x, right is -y.
        assert_eq!(light.origin, Vec3f::new(118.0, -16.0, 24.0));
        assert_eq!(light.radius, 205.0);
        assert_eq!(light.color, [1.0, 1.0, 0.0]);
        assert_eq!((light.minlight, light.die, light.key), (32.0, 500.0, 1));
        assert_eq!(names(&sounds), [("weapons/blastf1a.wav", CHAN_WEAPON, 0.0)]);
        assert_eq!((sounds[0].entnum, sounds[0].volume), (1, 1.0));
    }

    #[test]
    fn flash_lights_use_c_angle_vectors_double_trigonometry() {
        let mut fx = Effects::default();
        let mut mismatched = 0;
        for step in 0..720 {
            let angles = Vec3f::new(step as f32 * 0.37 - 89.0, step as f32 * 0.53, 0.0);
            let (forward, right, _) = q2_common::pmove::angle_vectors(angles);
            let player = EntityState {
                angles,
                ..player_at(1, 0.0)
            };
            fx.muzzle_flash(&player, MZ_BLASTER, 0.0, &mut rng(&[0]));
            let want = player.origin + forward * 18.0 + right * 16.0;
            mismatched += usize::from(fx.dlights().next().unwrap().origin != want);

            let offset = MONSTER_FLASH_OFFSET[MZ2_SOLDIER_BLASTER_1 as usize];
            fx.clear();
            fx.muzzle_flash2(&player, MZ2_SOLDIER_BLASTER_1, 0.0, &mut rng(&[0]));
            let mut want = player.origin + forward * offset[0] + right * offset[1];
            want.z += offset[2];
            mismatched += usize::from(fx.dlights().next().unwrap().origin != want);
            fx.clear();
        }
        assert_eq!(mismatched, 0);
    }

    #[test]
    fn mission_pack_player_flashes_light_and_sound_like_c() {
        for (weapon, color, sound, die) in [
            (
                MZ_BLUEHYPERBLASTER,
                [0.0, 0.0, 1.0],
                Some("weapons/hyprbf1a.wav"),
                500.0,
            ),
            (
                MZ_PHALANX,
                [1.0, 0.5, 0.5],
                Some("weapons/plasshot.wav"),
                500.0,
            ),
            (
                MZ_IONRIPPER,
                [1.0, 0.5, 0.5],
                Some("weapons/rippfire.wav"),
                500.0,
            ),
            (
                MZ_ETF_RIFLE,
                [0.9, 0.7, 0.0],
                Some("weapons/nail1.wav"),
                500.0,
            ),
            (
                MZ_SHOTGUN2,
                [1.0, 1.0, 0.0],
                Some("weapons/shotg2.wav"),
                500.0,
            ),
            (MZ_HEATBEAM, [1.0, 1.0, 0.0], None, 600.0),
            (
                MZ_BLASTER2,
                [0.0, 1.0, 0.0],
                Some("weapons/blastf1a.wav"),
                500.0,
            ),
            (
                MZ_TRACKER,
                [-1.0, -1.0, -1.0],
                Some("weapons/disint2.wav"),
                500.0,
            ),
            (MZ_NUKE1, [1.0, 0.0, 0.0], None, 600.0),
            (MZ_NUKE2, [1.0, 1.0, 0.0], None, 600.0),
            (MZ_NUKE4, [0.0, 0.0, 1.0], None, 600.0),
            (MZ_NUKE8, [0.0, 1.0, 1.0], None, 600.0),
        ] {
            let mut fx = Effects::default();
            let sounds = fx.muzzle_flash(
                &player_at(1, 0.0),
                weapon | MZ_SILENCED,
                500.0,
                &mut rng(&[0]),
            );
            let light = fx.dlights().next().unwrap();
            assert_eq!((light.color, light.die), (color, die), "{weapon}");
            match sound {
                Some(name) => {
                    assert_eq!(names(&sounds), [(name, CHAN_WEAPON, 0.0)], "{weapon}");
                    assert_eq!(sounds[0].volume, 0.2, "{weapon}");
                }
                None => assert!(sounds.is_empty(), "{weapon}"),
            }
        }
    }

    #[test]
    fn a_silenced_flash_is_smaller_and_quieter() {
        let mut fx = Effects::default();
        let sounds = fx.muzzle_flash(
            &player_at(1, 0.0),
            MZ_RAILGUN | MZ_SILENCED,
            0.0,
            &mut rng(&[0]),
        );
        let light = fx.dlights().next().unwrap();
        assert_eq!(light.radius, 100.0);
        assert_eq!(light.color, [0.5, 0.5, 1.0]);
        assert_eq!(sounds[0].volume, 0.2);
    }

    #[test]
    fn shotgun_rocket_and_grenade_flashes_add_a_delayed_second_sound() {
        let mut fx = Effects::default();
        let shotgun = fx.muzzle_flash(&player_at(1, 0.0), MZ_SHOTGUN, 0.0, &mut rng(&[0]));
        assert_eq!(
            names(&shotgun),
            [
                ("weapons/shotgf1b.wav", CHAN_WEAPON, 0.0),
                ("weapons/shotgr1b.wav", CHAN_AUTO, 0.1)
            ]
        );
        let rocket = fx.muzzle_flash(&player_at(1, 0.0), MZ_ROCKET, 0.0, &mut rng(&[0]));
        assert_eq!(rocket[1].name, "weapons/rocklr1b.wav");
        assert_eq!(fx.dlights().next().unwrap().color, [1.0, 0.5, 0.2]);
        let grenade = fx.muzzle_flash(&player_at(1, 0.0), MZ_GRENADE, 0.0, &mut rng(&[0]));
        assert_eq!(grenade[1].name, "weapons/grenlr1b.wav");
    }

    #[test]
    fn machinegun_flashes_pick_one_of_five_shots_at_random() {
        let mut fx = Effects::default();
        let sounds = fx.muzzle_flash(&player_at(1, 0.0), MZ_MACHINEGUN, 0.0, &mut rng(&[0, 3]));
        assert_eq!(sounds[0].name, "weapons/machgf4b.wav");
    }

    #[test]
    fn chaingun_flashes_grow_and_stack_shots_with_spin() {
        let mut fx = Effects::default();
        let one = fx.muzzle_flash(&player_at(1, 0.0), MZ_CHAINGUN1, 0.0, &mut rng(&[0]));
        assert_eq!(one.len(), 1);
        assert_eq!(fx.dlights().next().unwrap().color, [1.0, 0.25, 0.0]);
        let two = fx.muzzle_flash(&player_at(1, 0.0), MZ_CHAINGUN2, 0.0, &mut rng(&[0]));
        assert_eq!(
            two.iter().map(|s| s.timeofs).collect::<Vec<_>>(),
            [0.0, 0.05]
        );
        assert_eq!(fx.dlights().next().unwrap().radius, 225.0);
        let three = fx.muzzle_flash(&player_at(1, 0.0), MZ_CHAINGUN3, 0.0, &mut rng(&[0]));
        assert_eq!(
            three.iter().map(|s| s.timeofs).collect::<Vec<_>>(),
            [0.0, 0.033, 0.066]
        );
        let light = fx.dlights().next().unwrap();
        assert_eq!((light.radius, light.die), (250.0, 0.1));
    }

    #[test]
    fn hyper_ssg_and_bfg_flashes_use_their_sounds_and_colors() {
        let mut fx = Effects::default();
        for (weapon, sound, color) in [
            (MZ_HYPERBLASTER, "weapons/hyprbf1a.wav", [1.0, 1.0, 0.0]),
            (MZ_SSHOTGUN, "weapons/sshotf1b.wav", [1.0, 1.0, 0.0]),
            (MZ_BFG, "weapons/bfg__f1y.wav", [0.0, 1.0, 0.0]),
        ] {
            let sounds = fx.muzzle_flash(&player_at(1, 0.0), weapon, 0.0, &mut rng(&[0]));
            assert_eq!(sounds[0].name, sound);
            assert_eq!(fx.dlights().next().unwrap().color, color);
        }
    }

    #[test]
    fn login_flashes_last_a_moment_at_full_volume() {
        let mut fx = Effects::default();
        let sounds = fx.muzzle_flash(
            &player_at(1, 0.0),
            MZ_LOGIN | MZ_SILENCED,
            10.0,
            &mut rng(&[0]),
        );
        assert_eq!(sounds[0].volume, 1.0);
        let light = fx.dlights().next().unwrap();
        assert_eq!((light.color, light.die), ([0.0, 1.0, 0.0], 11.0));
        fx.muzzle_flash(&player_at(1, 0.0), MZ_LOGOUT, 10.0, &mut rng(&[0]));
        assert_eq!(fx.dlights().next().unwrap().color, [1.0, 0.0, 0.0]);
        // C CL_LogoutEffect at the player, by the flash without MZ_SILENCED.
        let origin = player_at(1, 0.0).origin;
        assert_eq!(
            fx.take_particles(),
            [
                (origin, FlashParticles::Logout(MZ_LOGIN)),
                (origin, FlashParticles::Logout(MZ_LOGOUT))
            ]
        );
        fx.muzzle_flash(&player_at(1, 0.0), MZ_ROCKET, 10.0, &mut rng(&[0]));
        assert!(fx.take_particles().is_empty());
    }

    #[test]
    fn a_monster_flash_lights_its_muzzle_and_plays_its_attack_sound() {
        let mut fx = Effects::default();
        let soldier = player_at(7, 90.0);
        let sounds = fx.muzzle_flash2(&soldier, MZ2_SOLDIER_BLASTER_1, 0.0, &mut rng(&[0]));
        let [f, r, u] = MONSTER_FLASH_OFFSET[MZ2_SOLDIER_BLASTER_1 as usize];
        // Facing +y: forward is +y, right is +x.
        let light = fx.dlights().next().unwrap();
        let want = Vec3f::new(100.0 + r, f, 24.0 + u);
        assert!(
            (light.origin - want).length() < 1e-3,
            "{:?} {want:?}",
            light.origin
        );
        assert_eq!(names(&sounds), [("soldier/solatck2.wav", CHAN_WEAPON, 0.0)]);
        assert_eq!(sounds[0].entnum, 7);
    }

    #[test]
    fn an_effect_light_takes_its_entitys_slot_like_cl_allocdlight() {
        let mut fx = Effects::default();
        let light = |origin, die| ClientDLight {
            key: 3,
            color: [1.0; 3],
            origin,
            radius: 400.0,
            die,
            decay: 0.0,
            minlight: 250.0,
        };
        fx.add_dlight(light(Vec3f::ZERO, 100.0), 0.0);
        fx.add_dlight(light(Vec3f::X, 110.0), 10.0);
        let lit: Vec<_> = fx.dlights().copied().collect();
        assert_eq!(lit, [light(Vec3f::X, 110.0)]);
    }

    #[test]
    fn monster_guns_queue_c_smoke_and_sparks_at_their_muzzle() {
        let mut fx = Effects::default();
        let soldier = player_at(7, 90.0);
        fx.muzzle_flash2(&soldier, MZ2_SOLDIER_MACHINEGUN_1, 0.0, &mut rng(&[0]));
        fx.muzzle_flash2(&soldier, MZ2_SOLDIER_SHOTGUN_1, 0.0, &mut rng(&[0]));
        fx.muzzle_flash2(&soldier, MZ2_SOLDIER_BLASTER_1, 0.0, &mut rng(&[0]));
        let muzzle = fx.dlights().next().unwrap().origin;
        let smoke = fx.take_particles();
        assert_eq!(
            smoke,
            [
                (muzzle, FlashParticles::SparksAndSmoke),
                (muzzle, FlashParticles::Smoke)
            ]
        );
        assert!(fx.take_particles().is_empty(), "taken once");
        for flash in [
            MZ2_SUPERTANK_MACHINEGUN_1,
            MZ2_JORG_MACHINEGUN_R6,
            MZ2_TURRET_MACHINEGUN,
        ] {
            fx.muzzle_flash2(&soldier, flash, 0.0, &mut rng(&[0]));
        }
        assert!(fx
            .take_particles()
            .iter()
            .all(|&(_, kind)| kind == FlashParticles::SparksAndSmoke));
    }

    #[test]
    fn tank_machineguns_leave_their_sound_to_the_particles_drawn_first() {
        let mut fx = Effects::default();
        let sounds = fx.muzzle_flash2(
            &player_at(7, 0.0),
            MZ2_TANK_MACHINEGUN_1,
            0.0,
            &mut rng(&[0]),
        );
        assert!(sounds.is_empty(), "C picks the letter after the sparks");
        let muzzle = fx.dlights().next().unwrap().origin;
        assert_eq!(
            fx.take_particles(),
            [(muzzle, FlashParticles::TankMachinegun(7))]
        );
    }

    #[test]
    fn boss_machineguns_are_heard_everywhere_and_some_flashes_are_silent() {
        let mut fx = Effects::default();
        let boss = fx.muzzle_flash2(
            &player_at(7, 0.0),
            MZ2_BOSS2_MACHINEGUN_L1,
            0.0,
            &mut rng(&[0]),
        );
        assert_eq!(boss[0].attenuation, ATTN_NONE);
        let rail = fx.muzzle_flash2(
            &player_at(7, 0.0),
            MZ2_GLADIATOR_RAILGUN_1,
            0.0,
            &mut rng(&[0]),
        );
        assert!(rail.is_empty());
        assert_eq!(fx.dlights().next().unwrap().color, [0.5, 0.5, 1.0]);
    }

    #[test]
    fn every_monster_flash_group_lights_and_sounds_like_c() {
        const YELLOW: [f32; 3] = [1.0, 1.0, 0.0];
        const ROCKET: [f32; 3] = [1.0, 0.5, 0.2];
        const BFG: [f32; 3] = [0.5, 1.0, 0.5];
        // C ref: CL_AddMuzzleFlash2's switch; the first and last case of
        // each group, so a case dropped from either end shows.
        // Flashes, light colour, and sound with its attenuation.
        type Group = (&'static [i32], [f32; 3], Option<(&'static str, f32)>);
        let groups: &[Group] = &[
            (
                &[MZ2_INFANTRY_MACHINEGUN_1, MZ2_INFANTRY_MACHINEGUN_13],
                YELLOW,
                Some(("infantry/infatck1.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_SOLDIER_MACHINEGUN_1, MZ2_SOLDIER_MACHINEGUN_8],
                YELLOW,
                Some(("soldier/solatck3.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_GUNNER_MACHINEGUN_1, MZ2_GUNNER_MACHINEGUN_8],
                YELLOW,
                Some(("gunner/gunatck2.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_ACTOR_MACHINEGUN_1, MZ2_TURRET_MACHINEGUN],
                YELLOW,
                Some(("infantry/infatck1.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_BOSS2_MACHINEGUN_L1, MZ2_CARRIER_MACHINEGUN_L2],
                YELLOW,
                Some(("infantry/infatck1.wav", ATTN_NONE)),
            ),
            (
                &[MZ2_SOLDIER_BLASTER_1, MZ2_TURRET_BLASTER],
                YELLOW,
                Some(("soldier/solatck2.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_FLYER_BLASTER_1, MZ2_FLYER_BLASTER_2],
                YELLOW,
                Some(("flyer/flyatck3.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_MEDIC_BLASTER_1],
                YELLOW,
                Some(("medic/medatck1.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_HOVER_BLASTER_1],
                YELLOW,
                Some(("hover/hovatck1.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_FLOAT_BLASTER_1],
                YELLOW,
                Some(("floater/fltatck1.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_SOLDIER_SHOTGUN_1, MZ2_SOLDIER_SHOTGUN_8],
                YELLOW,
                Some(("soldier/solatck1.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_TANK_BLASTER_1, MZ2_TANK_BLASTER_3],
                YELLOW,
                Some(("tank/tnkatck3.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_CHICK_ROCKET_1, MZ2_TURRET_ROCKET],
                ROCKET,
                Some(("chick/chkatck2.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_TANK_ROCKET_1, MZ2_TANK_ROCKET_3],
                ROCKET,
                Some(("tank/tnkatck1.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_SUPERTANK_ROCKET_1, MZ2_CARRIER_ROCKET_1],
                ROCKET,
                Some(("tank/rocket.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_GUNNER_GRENADE_1, MZ2_GUNNER_GRENADE_4],
                [1.0, 0.5, 0.0],
                Some(("gunner/gunatck3.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_GLADIATOR_RAILGUN_1, MZ2_WIDOW_RAIL],
                [0.5, 0.5, 1.0],
                None,
            ),
            (&[MZ2_MAKRON_BFG], BFG, None),
            (
                &[MZ2_MAKRON_BLASTER_1, MZ2_MAKRON_BLASTER_17],
                YELLOW,
                Some(("makron/blaster.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_JORG_MACHINEGUN_L1, MZ2_JORG_MACHINEGUN_L6],
                YELLOW,
                Some(("boss3/xfire.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_JORG_MACHINEGUN_R1, MZ2_JORG_MACHINEGUN_R6],
                YELLOW,
                None,
            ),
            (&[MZ2_JORG_BFG_1], BFG, None),
            (
                &[MZ2_BOSS2_MACHINEGUN_R1, MZ2_CARRIER_MACHINEGUN_R2],
                YELLOW,
                None,
            ),
            (
                &[MZ2_STALKER_BLASTER, MZ2_WIDOW_RUN_8],
                [0.0, 1.0, 0.0],
                Some(("tank/tnkatck3.wav", ATTN_NORM)),
            ),
            (
                &[MZ2_WIDOW_DISRUPTOR],
                [-1.0, -1.0, -1.0],
                Some(("weapons/disint2.wav", ATTN_NORM)),
            ),
        ];
        for &(flashes, color, sound) in groups {
            for &flash in flashes {
                let mut fx = Effects::default();
                let sounds = fx.muzzle_flash2(&player_at(7, 0.0), flash, 0.0, &mut rng(&[0]));
                let light = fx.dlights().next().unwrap();
                assert_eq!((light.color, light.die), (color, 0.0), "{flash}");
                let heard: Vec<_> = sounds
                    .iter()
                    .map(|s| (s.name.as_str(), s.entchannel, s.attenuation))
                    .collect();
                let want: Vec<_> = sound
                    .map(|(n, a)| (n, CHAN_WEAPON, a))
                    .into_iter()
                    .collect();
                assert_eq!(heard, want, "{flash}");
            }
        }
    }

    #[test]
    fn a_monster_flash_c_has_no_case_for_lights_without_changing_colour_or_sound() {
        let mut fx = Effects::default();
        let monster = player_at(7, 0.0);
        fx.muzzle_flash2(&monster, MZ2_WIDOW_DISRUPTOR, 0.0, &mut rng(&[0]));
        // C ref: CL_AddMuzzleFlash2 places the light, then its switch has
        // no case (and no default) for these numbers.
        for flash in [MZ2_MAKRON_RAILGUN_1, MZ2_CARRIER_GRENADE, 0] {
            let sounds = fx.muzzle_flash2(&monster, flash, 10.0, &mut rng(&[3]));
            assert!(sounds.is_empty(), "{flash}");
            let light = fx.dlights().next().unwrap();
            assert_eq!(light.color, [-1.0, -1.0, -1.0], "{flash}");
            assert_eq!((light.radius, light.die), (203.0, 10.0), "{flash}");
        }
        assert!(fx.take_particles().is_empty());
    }

    #[test]
    fn a_respawn_flash_lights_yellow_and_queues_its_logout_effect() {
        let mut fx = Effects::default();
        let sounds = fx.muzzle_flash(&player_at(1, 0.0), MZ_RESPAWN, 10.0, &mut rng(&[0]));
        let light = fx.dlights().next().unwrap();
        assert_eq!((light.color, light.die), ([1.0, 1.0, 0.0], 11.0));
        assert_eq!(names(&sounds), [("weapons/grenlf1a.wav", CHAN_WEAPON, 0.0)]);
        assert_eq!(
            fx.take_particles(),
            [(player_at(1, 0.0).origin, FlashParticles::Logout(MZ_RESPAWN))]
        );
    }

    #[test]
    fn a_player_flash_c_has_no_case_for_keeps_the_lights_colour_and_is_silent() {
        let mut fx = Effects::default();
        fx.muzzle_flash(&player_at(1, 0.0), MZ_RAILGUN, 10.0, &mut rng(&[0]));
        // 99 is no MZ_* number: C's switch has no case and no default.
        let sounds = fx.muzzle_flash(&player_at(1, 0.0), 99, 20.0, &mut rng(&[4]));
        assert!(sounds.is_empty());
        let light = fx.dlights().next().unwrap();
        assert_eq!(light.color, [0.5, 0.5, 1.0]);
        assert_eq!((light.radius, light.die), (204.0, 20.0));
    }

    #[test]
    fn the_widow_plasma_beam_lights_bigger_and_longer() {
        let mut fx = Effects::default();
        let sounds = fx.muzzle_flash2(
            &player_at(7, 0.0),
            MZ2_WIDOW_PLASMABEAM,
            50.0,
            &mut rng(&[0, 40]),
        );
        assert!(sounds.is_empty());
        let light = fx.dlights().next().unwrap();
        // C: 300 + (randk() & 100), a bitwise and: 40 & 100 = 32.
        assert_eq!((light.radius, light.die), (332.0, 250.0));
    }

    #[test]
    fn a_flash_number_past_the_table_does_nothing() {
        let mut fx = Effects::default();
        assert!(fx
            .muzzle_flash2(&player_at(7, 0.0), 211, 0.0, &mut rng(&[0]))
            .is_empty());
        assert_eq!(fx.dlights().count(), 0);
    }

    #[test]
    fn a_new_flash_from_the_same_entity_reuses_its_light() {
        let mut fx = Effects::default();
        fx.muzzle_flash(&player_at(1, 0.0), MZ_BLASTER, 100.0, &mut rng(&[0]));
        // A dead slot (die 0 < 100) takes another entity's flash.
        fx.muzzle_flash(&player_at(2, 0.0), MZ_BLASTER, 100.0, &mut rng(&[0]));
        assert_eq!(fx.dlights().count(), 2);
        fx.muzzle_flash(&player_at(1, 0.0), MZ_RAILGUN, 100.0, &mut rng(&[0]));
        assert_eq!(fx.dlights().count(), 2, "entity 1 reused its light");
        assert_eq!(fx.dlights[0].color, [0.5, 0.5, 1.0]);
    }

    #[test]
    fn a_muzzle_flash_message_is_read_against_the_entity_it_names() {
        use q2_shared::protocol::SvcOp;
        let mut fx = Effects::default();
        let me = player_at(1, 0.0);
        let entities = |n: i32| (n == 1).then(|| me.clone());
        let player_msg = [SvcOp::MuzzleFlash as u8, 1, 0, MZ_SHOTGUN as u8];
        let sounds = fx.message(&player_msg, entities, 0.0, &mut rng(&[0]));
        assert_eq!(sounds.len(), 2);
        let monster_msg = [SvcOp::MuzzleFlash2 as u8, 1, 0, MZ2_SOLDIER_BLASTER_1 as u8];
        let sounds = fx.message(&monster_msg, entities, 0.0, &mut rng(&[0]));
        assert_eq!(sounds[0].name, "soldier/solatck2.wav");
    }

    #[test]
    fn other_messages_and_unknown_entities_are_ignored() {
        use q2_shared::protocol::SvcOp;
        let mut fx = Effects::default();
        let nobody = |_: i32| None;
        let flash = [SvcOp::MuzzleFlash as u8, 9, 0, MZ_BLASTER as u8];
        assert!(fx.message(&flash, nobody, 0.0, &mut rng(&[0])).is_empty());
        let temp = [SvcOp::TempEntity as u8, 5];
        assert!(fx.message(&temp, nobody, 0.0, &mut rng(&[0])).is_empty());
        assert!(fx.message(&[], nobody, 0.0, &mut rng(&[0])).is_empty());
        assert_eq!(fx.dlights().count(), 0);
    }

    #[test]
    fn a_flash_cut_short_or_naming_a_bad_entity_does_nothing() {
        use q2_shared::protocol::SvcOp;
        let mut fx = Effects::default();
        let anyone = |n: i32| Some(player_at(n, 0.0));
        let mut draws = 0;
        let mut randk = || {
            draws += 1;
            0
        };
        for op in [SvcOp::MuzzleFlash, SvcOp::MuzzleFlash2] {
            // Cut after the entity number: no flash byte.
            let short = [op as u8, 1, 0];
            assert!(fx.message(&short, anyone, 0.0, &mut randk).is_empty());
            // C Com_Error "bad entity" outside 1..MAX_EDICTS-1.
            for number in [0, MAX_EDICTS as i16, -1] {
                let [lo, hi] = number.to_le_bytes();
                let flash = [op as u8, lo, hi, MZ_BLASTER as u8];
                assert!(fx.message(&flash, anyone, 0.0, &mut randk).is_empty());
            }
        }
        assert_eq!(fx.dlights().count(), 0);
        assert!(fx.take_particles().is_empty());
        assert_eq!(draws, 0);
    }

    #[test]
    fn lights_decay_and_go_out_32_ms_after_they_die() {
        let mut fx = Effects::default();
        fx.muzzle_flash(&player_at(1, 0.0), MZ_BLASTER, 100.0, &mut rng(&[0]));
        fx.dlights[0].decay = 100.0;
        fx.run_dlights(132.0, 0.5);
        assert_eq!(fx.dlights().next().unwrap().radius, 150.0);
        fx.run_dlights(132.0, 10.0);
        assert_eq!(fx.dlights().count(), 0, "decayed to zero");

        fx.muzzle_flash(&player_at(1, 0.0), MZ_BLASTER, 100.0, &mut rng(&[0]));
        fx.run_dlights(133.0, 0.0);
        assert_eq!(fx.dlights().count(), 0, "past its time");
        fx.clear();
        assert_eq!(fx.dlights().count(), 0);
    }
}
