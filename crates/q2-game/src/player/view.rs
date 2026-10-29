//! Player view — camera offset, view bob, damage kicks, fall effects.
//! C ref: `player/view.c` (1,426 lines)

use q2_shared::types::*;

use q2_shared::constants::{
    CONTENTS_LAVA, CONTENTS_SLIME, CONTENTS_SOLID, CONTENTS_WATER, EF_COLOR_SHELL, EF_PENT,
    EF_POWERSCREEN, EF_QUAD, RDF_UNDERWATER, RF_IR_VISIBLE, RF_SHELL_BLUE, RF_SHELL_GREEN,
    RF_SHELL_RED, STAT_FLASHES,
};

use crate::constants::{
    DamageFlags, EntityFlags, MeansOfDeath, ANIM_PAIN, ATTN_NORM, CHAN_AUTO, CHAN_BODY, CHAN_ITEM,
    CHAN_VOICE, DAMAGE_TIME, POWER_ARMOR_SCREEN, POWER_ARMOR_SHIELD,
};

// Player pain frames (C `player.h`).
const FRAME_PAIN101: i32 = 54;
const FRAME_PAIN104: i32 = 57;
const FRAME_PAIN201: i32 = 58;
const FRAME_PAIN204: i32 = 61;
const FRAME_PAIN301: i32 = 62;
const FRAME_PAIN304: i32 = 65;
const FRAME_CRPAIN1: i32 = 169;
const FRAME_CRPAIN4: i32 = 172;
use crate::entity::EntityKey;
use crate::world::GameWorld;

impl GameWorld {
    /// Calculate view offset for a player entity.
    /// Applies view bob, damage kick, and fall effects.
    /// C ref: `SV_CalcViewOffset` (player/view.c).
    pub fn update_player_view(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get(key) else {
            return;
        };
        if ent.client.is_none() {
            return;
        }

        // View bob based on movement speed.
        let speed = Vec3f::new(ent.velocity.x, ent.velocity.y, 0.0).length();
        let bob_time = self.level.time;
        let bob = if speed > 0.0 {
            (bob_time * 6.0).sin() * speed * 0.005
        } else {
            0.0
        };

        // Apply view offset to player state: eye height comes from the
        // entity's viewheight (C SV_CalcViewOffset), which pmove lowers when
        // ducking.
        // The last shot's kick (C: base angles are `kick_angles`, cleared
        // for the dead; the origin kick adds to the eye, clamped).
        let now = self.level.time;
        if let Some((ent, client)) = self.entity_and_client_mut(key) {
            client.ps.kick_angles = if ent.game.deadflag != crate::constants::DeadFlag::No {
                Vec3f::ZERO
            } else {
                // Plus the damage kick, fading over DAMAGE_TIME.
                let ratio = ((client.v_dmg_time - now) / DAMAGE_TIME).max(0.0);
                client.kick_angles
                    + Vec3f::new(ratio * client.v_dmg_pitch, 0.0, ratio * client.v_dmg_roll)
            };
            let v = Vec3f::new(0.0, 0.0, ent.game.viewheight + bob) + client.kick_origin;
            client.ps.viewoffset = Vec3f::new(
                v.x.clamp(-14.0, 14.0),
                v.y.clamp(-14.0, 14.0),
                v.z.clamp(-22.0, 30.0),
            );
        }
    }
}

/// This frame's walk bob (C `ClientEndServerFrame`'s file statics).
#[derive(Debug, Clone, Copy)]
pub struct Bob {
    /// Horizontal speed.
    pub xyspeed: f32,
    /// Whole bob cycles; odd ones swing the other way.
    pub cycle: i32,
    /// `|sin(bobtime * pi)|`.
    pub fracsin: f32,
}

/// Mix `rgb` at alpha `a` over `blend`. C ref: `SV_AddBlend`.
pub fn add_blend(rgb: [f32; 3], a: f32, blend: &mut [f32; 4]) {
    if a <= 0.0 {
        return;
    }
    let a2 = blend[3] + (1.0 - blend[3]) * a; // new total alpha
    let a3 = blend[3] / a2; // fraction of color from old
    for i in 0..3 {
        blend[i] = blend[i] * a3 + rgb[i] * (1.0 - a3);
    }
    blend[3] = a2;
}

impl GameWorld {
    /// The full-screen tint: what the eye is in, the running powerup
    /// (with its warning sound three seconds before it runs out) and the
    /// damage and pickup flashes. C ref: `SV_CalcBlend` (player/view.c).
    pub fn calc_blend(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get(key) else {
            return;
        };
        let origin = ent.state.origin;
        let Some(client) = self.client(key) else {
            return;
        };
        let contents = self.gi.point_contents(origin + client.ps.viewoffset);
        let framenum = self.level.framenum;
        let powerup = [
            (
                client.quad_framenum,
                "items/damage2.wav",
                [0.0, 0.0, 1.0],
                0.08,
            ),
            (
                client.invincible_framenum,
                "items/protect2.wav",
                [1.0, 1.0, 0.0],
                0.08,
            ),
            (
                client.enviro_framenum,
                "items/airout.wav",
                [0.0, 1.0, 0.0],
                0.08,
            ),
            (
                client.breather_framenum,
                "items/airout.wav",
                [0.4, 1.0, 0.4],
                0.04,
            ),
        ]
        .into_iter()
        .find(|&(until, ..)| until > framenum);
        if let Some((until, warning, ..)) = powerup {
            if until - framenum == 30 {
                // Beginning to fade.
                let sound = self.gi.sound_index(warning);
                self.gi
                    .sound(self.number_of(key), CHAN_ITEM, sound, 1.0, ATTN_NORM, 0.0);
            }
        }
        let Some(client) = self.client_mut(key) else {
            return;
        };
        let blend = &mut client.ps.blend;
        *blend = [0.0; 4];
        if contents & (CONTENTS_LAVA | CONTENTS_SLIME | CONTENTS_WATER) != 0 {
            client.ps.rdflags |= RDF_UNDERWATER;
        } else {
            client.ps.rdflags &= !RDF_UNDERWATER;
        }
        if contents & (CONTENTS_SOLID | CONTENTS_LAVA) != 0 {
            add_blend([1.0, 0.3, 0.0], 0.6, blend);
        } else if contents & CONTENTS_SLIME != 0 {
            add_blend([0.0, 0.1, 0.05], 0.6, blend);
        } else if contents & CONTENTS_WATER != 0 {
            add_blend([0.5, 0.3, 0.2], 0.4, blend);
        }
        if let Some((until, _, rgb, alpha)) = powerup {
            let remaining = until - framenum;
            if remaining > 30 || remaining & 4 != 0 {
                add_blend(rgb, alpha, blend);
            }
        }
        if client.damage_alpha > 0.0 {
            add_blend(client.damage_blend, client.damage_alpha, blend);
        }
        if client.bonus_alpha > 0.0 {
            add_blend([0.85, 0.7, 0.3], client.bonus_alpha, blend);
        }
        client.damage_alpha = (client.damage_alpha - 0.06).max(0.0);
        client.bonus_alpha = (client.bonus_alpha - 0.1).max(0.0);
    }

    /// Water, slime and lava on the player: splash sounds going in and
    /// out, gasps, drowning once a second after the air runs out (the
    /// rebreather and envirosuit give air), and burning. C ref:
    /// `P_WorldEffects` (player/view.c).
    pub fn world_effects(&mut self, key: EntityKey) {
        use crate::ai::world_damage;
        use crate::player::weapon::PlayerNoise;
        let (time, framenum) = (self.level.time, self.level.framenum);
        let Some(ent) = self.entities.get(key) else {
            return;
        };
        let (origin, waterlevel, watertype) =
            (ent.state.origin, ent.game.waterlevel, ent.game.watertype);
        let noclip = ent.game.movetype == crate::constants::MoveType::Noclip;
        let Some(client) = self.client_mut(key) else {
            return;
        };
        if noclip {
            client.air_finished = time + 12.0; // don't need air
            return;
        }
        let old_waterlevel = std::mem::replace(&mut client.old_waterlevel, waterlevel);
        let breather = client.breather_framenum > framenum;
        let envirosuit = client.enviro_framenum > framenum;
        let breather_left = client.breather_framenum - framenum;
        let air_finished = client.air_finished;
        let number = self.number_of(key);
        let sound = |world: &GameWorld, channel, name: &str| {
            let index = world.gi.sound_index(name);
            world.gi.sound(number, channel, index, 1.0, ATTN_NORM, 0.0);
        };

        // Just entered a water volume.
        if old_waterlevel == 0 && waterlevel != 0 {
            self.player_noise(key, origin, PlayerNoise::SelfNoise);
            if watertype & CONTENTS_LAVA != 0 {
                sound(self, CHAN_BODY, "player/lava_in.wav");
            } else if watertype & (CONTENTS_SLIME | CONTENTS_WATER) != 0 {
                sound(self, CHAN_BODY, "player/watr_in.wav");
            }
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.flags |= EntityFlags::INWATER;
                // So the pain sound plays at once.
                ent.game.damage_debounce_time = time - 1.0;
            }
        }
        // Just completely left a water volume.
        if old_waterlevel != 0 && waterlevel == 0 {
            self.player_noise(key, origin, PlayerNoise::SelfNoise);
            sound(self, CHAN_BODY, "player/watr_out.wav");
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.flags.remove(EntityFlags::INWATER);
            }
        }
        // Head just going under, or just coming out.
        if old_waterlevel != 3 && waterlevel == 3 {
            sound(self, CHAN_BODY, "player/watr_un.wav");
        }
        if old_waterlevel == 3 && waterlevel != 3 {
            if air_finished < time {
                sound(self, CHAN_VOICE, "player/gasp1.wav");
                self.player_noise(key, origin, PlayerNoise::SelfNoise);
            } else if air_finished < time + 11.0 {
                sound(self, CHAN_VOICE, "player/gasp2.wav");
            }
        }

        if waterlevel == 3 {
            if breather || envirosuit {
                if let Some(client) = self.client_mut(key) {
                    client.air_finished = time + 10.0;
                }
                if breather_left % 25 == 0 {
                    let second = self.client(key).is_some_and(|c| c.breather_sound);
                    let name = if second {
                        "player/u_breath2.wav"
                    } else {
                        "player/u_breath1.wav"
                    };
                    sound(self, CHAN_AUTO, name);
                    if let Some(client) = self.client_mut(key) {
                        client.breather_sound = !second;
                    }
                    self.player_noise(key, origin, PlayerNoise::SelfNoise);
                }
            }
            let drowning = self
                .client(key)
                .is_some_and(|c| c.air_finished < time && c.next_drown_time < time);
            let health = self.entities.get(key).map_or(0, |e| e.game.health);
            if drowning && health > 0 {
                if let Some(client) = self.client_mut(key) {
                    client.next_drown_time = time + 1.0;
                }
                // Take more damage the longer under water.
                let Some(ent) = self.entities.get_mut(key) else {
                    return;
                };
                ent.game.dmg = (ent.game.dmg + 2).min(15);
                let dmg = ent.game.dmg;
                ent.game.pain_debounce_time = time;
                // A gurp instead of the normal pain sound.
                let gurp = if health <= dmg {
                    "player/drown1.wav"
                } else if self.random_u32() & 1 != 0 {
                    "*gurp1.wav"
                } else {
                    "*gurp2.wav"
                };
                sound(self, CHAN_VOICE, gurp);
                world_damage(self, key, dmg, DamageFlags::NO_ARMOR, MeansOfDeath::Water);
            }
        } else {
            if let Some(client) = self.client_mut(key) {
                client.air_finished = time + 12.0;
            }
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.dmg = 2;
            }
        }

        // Sizzle damage.
        if waterlevel != 0 && watertype & CONTENTS_LAVA != 0 {
            let invincible = self
                .client(key)
                .is_some_and(|c| c.invincible_framenum >= framenum);
            let Some(ent) = self.entities.get(key) else {
                return;
            };
            let burns = ent.game.health > 0
                && ent.game.pain_debounce_time <= time
                && !invincible
                && !ent.game.flags.contains(EntityFlags::GODMODE);
            if burns {
                let name = if self.random_u32() & 1 != 0 {
                    "player/burn1.wav"
                } else {
                    "player/burn2.wav"
                };
                sound(self, CHAN_VOICE, name);
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.game.pain_debounce_time = time + 1.0;
                }
            }
            // A third of the damage with the envirosuit.
            let per_level = if envirosuit { 1 } else { 3 };
            world_damage(
                self,
                key,
                per_level * waterlevel,
                DamageFlags::empty(),
                MeansOfDeath::Lava,
            );
        }
        if waterlevel != 0 && watertype & CONTENTS_SLIME != 0 && !envirosuit {
            world_damage(
                self,
                key,
                waterlevel,
                DamageFlags::empty(),
                MeansOfDeath::Slime,
            );
        }
    }

    /// Turn this frame's damage into feedback: the death or gib sound, the
    /// status-number flashes, a pain animation and sound (by health, at
    /// most every 0.7 s, none in god mode or invulnerable), the damage
    /// tint mixed by what absorbed the hit, and a view kick away from
    /// where it came from. C ref: `P_DamageFeedback` (player/view.c).
    pub fn damage_feedback(&mut self, key: EntityKey) {
        const POWER_COLOR: [f32; 3] = [0.0, 1.0, 0.0];
        const ARMOR_COLOR: [f32; 3] = [1.0, 1.0, 1.0];
        const BLOOD_COLOR: [f32; 3] = [1.0, 0.0, 0.0];
        let (time, framenum) = (self.level.time, self.level.framenum);
        let number = self.number_of(key);
        // Death/gib sound is aggregated and played here.
        let death_sound = self
            .client_mut(key)
            .map_or(0, |c| std::mem::take(&mut c.death_sound));
        if death_sound != 0 {
            self.gi
                .sound(number, CHAN_VOICE, death_sound, 1.0, ATTN_NORM, 0.0);
        }
        let Some(ent) = self.entities.get(key) else {
            return;
        };
        let godmode = ent.game.flags.contains(EntityFlags::GODMODE);
        let (health, origin, modelindex, pain_debounce) = (
            ent.game.health,
            ent.state.origin,
            ent.state.modelindex,
            ent.game.pain_debounce_time,
        );
        let Some(client) = self.client_mut(key) else {
            return;
        };
        let invincible = client.invincible_framenum > framenum;
        // Flash the backgrounds behind the status numbers.
        let mut flashes = 0;
        if client.damage_blood != 0 {
            flashes |= 1;
        }
        if client.damage_armor != 0 && !godmode && !invincible {
            flashes |= 2;
        }
        client.ps.stats[STAT_FLASHES] = flashes;
        // Total points of damage shot at the player this frame.
        let count = client.damage_blood + client.damage_armor + client.damage_parmor;
        if count == 0 {
            return; // didn't take any damage
        }
        let (blood, armor, parmor, knockback, from) = (
            client.damage_blood,
            client.damage_armor,
            client.damage_parmor,
            client.damage_knockback,
            client.damage_from,
        );
        let ducked = client.ps.pmove.pm_flags & q2_common::pmove::PMF_DUCKED != 0;
        let v_angle = client.v_angle;

        // Start a pain animation if still in the player model.
        if client.anim_priority < ANIM_PAIN && modelindex == 255 {
            self.pain_anim_cycle = (self.pain_anim_cycle + 1) % 3;
            let (first, last) = if ducked {
                (FRAME_CRPAIN1, FRAME_CRPAIN4)
            } else {
                [
                    (FRAME_PAIN101, FRAME_PAIN104),
                    (FRAME_PAIN201, FRAME_PAIN204),
                    (FRAME_PAIN301, FRAME_PAIN304),
                ][self.pain_anim_cycle as usize]
            };
            if let Some((ent, client)) = self.entity_and_client_mut(key) {
                client.anim_priority = ANIM_PAIN;
                ent.state.frame = first - 1;
                client.anim_end = last;
            }
        }

        let realcount = count as f32;
        let count = (count as f32).max(10.0); // always make a visible effect

        // Play an appropriate pain sound.
        if time > pain_debounce && !godmode && !invincible && health > 0 {
            let r = 1 + (self.random_u32() & 1);
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.pain_debounce_time = time + 0.7;
            }
            let level = match health {
                ..25 => 25,
                25..50 => 50,
                50..75 => 75,
                _ => 100,
            };
            let pain = self.gi.sound_index(&format!("*pain{level}_{r}.wav"));
            self.gi.sound(number, CHAN_VOICE, pain, 1.0, ATTN_NORM, 0.0);
        }

        let Some(client) = self.client_mut(key) else {
            return;
        };
        // The total alpha of the blend is always proportional to count.
        client.damage_alpha = (client.damage_alpha.max(0.0) + count * 0.01).clamp(0.2, 0.6);
        // The colour varies with how much each armor absorbed.
        let mut blend = [0.0; 3];
        for (amount, color) in [
            (parmor, POWER_COLOR),
            (armor, ARMOR_COLOR),
            (blood, BLOOD_COLOR),
        ] {
            if amount != 0 {
                let share = amount as f32 / realcount;
                (0..3).for_each(|i| blend[i] += share * color[i]);
            }
        }
        client.damage_blend = blend;

        // View angle kicks; a kick of 0 means no view adjust at all.
        let kick = knockback.abs() as f32;
        if kick != 0.0 && health > 0 {
            let kick = (kick * 100.0 / health as f32).max(count * 0.5).min(50.0);
            let v = (from - origin).normalize_or_zero();
            let (forward, right, _) = crate::physics::angle_vectors(v_angle);
            client.v_dmg_roll = kick * v.dot(right) * 0.3;
            client.v_dmg_pitch = kick * -v.dot(forward) * 0.3;
            client.v_dmg_time = time + DAMAGE_TIME;
        }

        // Clear totals.
        client.damage_blood = 0;
        client.damage_armor = 0;
        client.damage_parmor = 0;
        client.damage_knockback = 0;
    }

    /// Advance the walk bob: a quicker step at higher speed on the ground,
    /// none in the air, back to the start when nearly still; ducked steps
    /// cycle four times as fast. C ref: `ClientEndServerFrame`'s bob part.
    pub fn calc_bob(&mut self, key: EntityKey) -> Bob {
        let Some(ent) = self.entities.get(key) else {
            return Bob {
                xyspeed: 0.0,
                cycle: 0,
                fracsin: 0.0,
            };
        };
        let xyspeed = Vec3f::new(ent.velocity.x, ent.velocity.y, 0.0).length();
        let on_ground = ent.game.ground_entity.is_some();
        let Some(client) = self.client_mut(key) else {
            return Bob {
                xyspeed,
                cycle: 0,
                fracsin: 0.0,
            };
        };
        let bobmove = if xyspeed < 5.0 {
            client.bobtime = 0.0; // start at beginning of cycle again
            0.0
        } else if on_ground {
            if xyspeed > 210.0 {
                0.25
            } else if xyspeed > 100.0 {
                0.125
            } else {
                0.0625
            }
        } else {
            0.0
        };
        client.bobtime += bobmove;
        let mut bobtime = client.bobtime;
        if client.ps.pmove.pm_flags & q2_common::pmove::PMF_DUCKED != 0 {
            bobtime *= 4.0;
        }
        Bob {
            xyspeed,
            cycle: bobtime as i32,
            fracsin: (bobtime * std::f32::consts::PI).sin().abs(),
        }
    }

    /// Sway the gun with the walk bob and lag it behind turns. The
    /// `gun_x/y/z` development offsets are 0, so `gunoffset` stays zero.
    /// C ref: `SV_CalcGunOffset` (player/view.c).
    pub fn calc_gun_offset(&mut self, key: EntityKey, bob: &Bob) {
        let Some(client) = self.client_mut(key) else {
            return;
        };
        let sway = bob.xyspeed * bob.fracsin;
        let side = if bob.cycle & 1 != 0 { -1.0 } else { 1.0 };
        let mut angles = Vec3f::new(sway * 0.005, side * sway * 0.01, side * sway * 0.005);
        // Gun angles from delta movement.
        for i in 0..3 {
            let mut delta = client.oldviewangles[i] - client.ps.viewangles[i];
            if delta > 180.0 {
                delta -= 360.0;
            }
            if delta < -180.0 {
                delta += 360.0;
            }
            let delta = delta.clamp(-45.0, 45.0);
            if i == 1 {
                angles.z += 0.1 * delta;
            }
            angles[i] += 0.2 * delta;
        }
        client.ps.gunangles = angles;
        client.ps.gunoffset = Vec3f::ZERO;
    }

    /// The player's glows: power armor while it absorbs, quad and
    /// invulnerability (blinking in their last three seconds), and god
    /// mode's white shell; none when dead or in intermission. C ref:
    /// `G_SetClientEffects` (player/view.c).
    pub fn set_client_effects(&mut self, key: EntityKey) {
        let (time, framenum) = (self.level.time, self.level.framenum);
        let intermission = self.level.intermission_time != 0.0;
        let power_armor = self.power_armor_type(key);
        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        ent.state.effects = 0;
        ent.state.renderfx = RF_IR_VISIBLE;
        if ent.game.health <= 0 || intermission {
            return;
        }
        if ent.game.powerarmor_time > time {
            if power_armor == POWER_ARMOR_SCREEN {
                ent.state.effects |= EF_POWERSCREEN;
            } else if power_armor == POWER_ARMOR_SHIELD {
                ent.state.effects |= EF_COLOR_SHELL;
                ent.state.renderfx |= RF_SHELL_GREEN;
            }
        }
        let glows = |until: i32| {
            let remaining = until - framenum;
            until > framenum && (remaining > 30 || remaining & 4 != 0)
        };
        if glows(client.quad_framenum) {
            ent.state.effects |= EF_QUAD;
        }
        if glows(client.invincible_framenum) {
            ent.state.effects |= EF_PENT;
        }
        // Show cheaters.
        if ent.game.flags.contains(EntityFlags::GODMODE) {
            ent.state.effects |= EF_COLOR_SHELL;
            ent.state.renderfx |= RF_SHELL_RED | RF_SHELL_GREEN | RF_SHELL_BLUE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::add_blend;
    use crate::constants::DAMAGE_TIME;
    use crate::entity::EntityKey;
    use crate::world::{test_world, GameWorld};
    use q2_shared::types::Vec3f;

    #[test]
    fn a_shot_kicks_the_view_for_one_frame_like_c() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        let client = world.client_mut(key).unwrap();
        client.kick_angles = Vec3f::new(-1.0, 0.0, 0.0);
        client.kick_origin = Vec3f::new(-2.0, 0.0, 0.0);

        world.client_end_server_frames();
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.kick_angles, Vec3f::new(-1.0, 0.0, 0.0));
        assert_eq!(client.ps.viewoffset.x, -2.0);
        assert_eq!(
            client.kick_angles,
            Vec3f::ZERO,
            "ClientEndServerFrame clears it"
        );
        assert_eq!(client.kick_origin, Vec3f::ZERO);

        world.client_end_server_frames();
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.kick_angles, Vec3f::ZERO);
        assert_eq!(client.ps.viewoffset.x, 0.0);
    }

    #[test]
    fn the_kick_offset_is_clamped_like_sv_calcviewoffset() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        world.client_mut(key).unwrap().kick_origin = Vec3f::new(-30.0, 30.0, 40.0);
        world.update_player_view(key);
        let offset = world.client(key).unwrap().ps.viewoffset;
        assert_eq!((offset.x, offset.y, offset.z), (-14.0, 14.0, 30.0));
    }

    #[test]
    fn a_damage_kick_fades_over_damage_time_like_c() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        world.level.time = 10.0;
        let client = world.client_mut(key).unwrap();
        client.v_dmg_pitch = -40.0;
        client.v_dmg_roll = 4.0;
        client.v_dmg_time = 10.0 + DAMAGE_TIME;
        world.update_player_view(key);
        let kick = world.client(key).unwrap().ps.kick_angles;
        assert_eq!((kick.x, kick.z), (-40.0, 4.0));

        world.level.time = 10.0 + DAMAGE_TIME / 2.0;
        world.update_player_view(key);
        assert_eq!(world.client(key).unwrap().ps.kick_angles.x, -20.0);

        world.level.time = 11.0;
        world.update_player_view(key);
        assert_eq!(
            world.client(key).unwrap().ps.kick_angles.x,
            0.0,
            "past: no kick"
        );
    }

    use q2_shared::constants::*;

    fn player_in(world: &mut GameWorld) -> EntityKey {
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        world.level.framenum = 100;
        key
    }

    fn effects(world: &GameWorld, key: EntityKey) -> (u32, i32) {
        let ent = world.entities.get(key).unwrap();
        (ent.state.effects, ent.state.renderfx)
    }

    #[test]
    fn quad_and_invulnerability_glow_and_blink_in_their_last_3_seconds() {
        let mut world = test_world();
        let key = player_in(&mut world);
        world.client_mut(key).unwrap().quad_framenum = 131;
        world.set_client_effects(key);
        assert_eq!(effects(&world, key), (EF_QUAD, RF_IR_VISIBLE));
        // remaining 30: & 4 is 4, on; remaining 27: & 4 is 0, off.
        world.client_mut(key).unwrap().quad_framenum = 130;
        world.set_client_effects(key);
        assert_eq!(effects(&world, key).0, EF_QUAD);
        world.client_mut(key).unwrap().quad_framenum = 127;
        world.set_client_effects(key);
        assert_eq!(effects(&world, key).0, 0);

        world.client_mut(key).unwrap().invincible_framenum = 200;
        world.set_client_effects(key);
        assert_eq!(effects(&world, key).0, EF_PENT);
    }

    #[test]
    fn absorbing_power_armor_shows_a_screen_or_a_green_shell() {
        let mut world = test_world();
        world.init_items();
        let key = player_in(&mut world);
        world.level.time = 5.0;
        let cells = world.find_item_by_name("Cells").unwrap();
        let screen = world.find_item_by_name("Power Screen").unwrap();
        let shield = world.find_item_by_name("Power Shield").unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.flags |= crate::constants::EntityFlags::POWER_ARMOR;
        ent.game.powerarmor_time = 5.2;
        let client = world.client_mut(key).unwrap();
        client.pers.inventory[cells] = 10;
        client.pers.inventory[screen] = 1;
        world.set_client_effects(key);
        assert_eq!(effects(&world, key).0, EF_POWERSCREEN);
        world.client_mut(key).unwrap().pers.inventory[shield] = 1;
        world.set_client_effects(key);
        assert_eq!(
            effects(&world, key),
            (EF_COLOR_SHELL, RF_IR_VISIBLE | RF_SHELL_GREEN)
        );
        world.level.time = 6.0;
        world.set_client_effects(key);
        assert_eq!(effects(&world, key).0, 0, "only while it absorbs");
    }

    #[test]
    fn god_mode_shows_a_white_shell_and_the_dead_show_nothing() {
        let mut world = test_world();
        let key = player_in(&mut world);
        world.entities.get_mut(key).unwrap().game.flags |= crate::constants::EntityFlags::GODMODE;
        world.set_client_effects(key);
        assert_eq!(
            effects(&world, key),
            (
                EF_COLOR_SHELL,
                RF_IR_VISIBLE | RF_SHELL_RED | RF_SHELL_GREEN | RF_SHELL_BLUE
            )
        );
        world.entities.get_mut(key).unwrap().game.health = 0;
        world.client_mut(key).unwrap().quad_framenum = 500;
        world.set_client_effects(key);
        assert_eq!(effects(&world, key), (0, RF_IR_VISIBLE));
    }

    #[test]
    fn add_blend_mixes_colors_by_alpha_like_sv_addblend() {
        let mut blend = [0.0; 4];
        add_blend([1.0, 0.0, 0.0], 0.5, &mut blend);
        assert_eq!(blend, [1.0, 0.0, 0.0, 0.5]);
        add_blend([0.0, 0.0, 1.0], 0.5, &mut blend);
        // a2 = 0.5 + 0.5 * 0.5 = 0.75; a3 = 0.5 / 0.75 = 2/3 old.
        assert!((blend[0] - 2.0 / 3.0).abs() < 1e-6);
        assert!((blend[2] - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(blend[3], 0.75);
        add_blend([0.0, 1.0, 0.0], 0.0, &mut blend);
        assert_eq!(blend[3], 0.75, "no alpha, no change");
    }

    #[test]
    fn the_view_tints_under_lava_slime_and_water_and_flags_underwater() {
        for (contents, rgba) in [
            (CONTENTS_LAVA, [1.0, 0.3, 0.0, 0.6]),
            (CONTENTS_SLIME, [0.0, 0.1, 0.05, 0.6]),
            (CONTENTS_WATER, [0.5, 0.3, 0.2, 0.4]),
        ] {
            let (mut world, gi) = crate::world::test_world_recording();
            let key = player_in(&mut world);
            gi.contents.set(contents);
            world.calc_blend(key);
            let client = world.client(key).unwrap();
            assert_eq!(client.ps.blend, rgba);
            assert_eq!(client.ps.rdflags & RDF_UNDERWATER, RDF_UNDERWATER);
        }
        let (mut world, _) = crate::world::test_world_recording();
        let key = player_in(&mut world);
        world.client_mut(key).unwrap().ps.rdflags = RDF_UNDERWATER;
        world.calc_blend(key);
        let client = world.client(key).unwrap();
        assert_eq!((client.ps.blend, client.ps.rdflags), ([0.0; 4], 0));
    }

    #[test]
    fn powerups_tint_the_view_and_warn_three_seconds_before_running_out() {
        let (mut world, gi) = crate::world::test_world_recording();
        let key = player_in(&mut world);
        world.client_mut(key).unwrap().quad_framenum = 130;
        world.calc_blend(key);
        assert_eq!(world.client(key).unwrap().ps.blend, [0.0, 0.0, 1.0, 0.08]);
        let fading = world.gi.sound_index("items/damage2.wav");
        assert!(gi.sound_calls.borrow().iter().any(|c| c.2 == fading));

        for (set, rgb, alpha, warning) in [
            (1, [1.0, 1.0, 0.0], 0.08, "items/protect2.wav"),
            (2, [0.0, 1.0, 0.0], 0.08, "items/airout.wav"),
            (3, [0.4, 1.0, 0.4], 0.04, "items/airout.wav"),
        ] {
            let (mut world, gi) = crate::world::test_world_recording();
            let key = player_in(&mut world);
            let client = world.client_mut(key).unwrap();
            match set {
                1 => client.invincible_framenum = 130,
                2 => client.enviro_framenum = 130,
                _ => client.breather_framenum = 130,
            }
            world.calc_blend(key);
            let blend = world.client(key).unwrap().ps.blend;
            assert_eq!(blend, [rgb[0], rgb[1], rgb[2], alpha], "{warning}");
            let index = world.gi.sound_index(warning);
            assert!(gi.sound_calls.borrow().iter().any(|c| c.2 == index));
        }
    }

    #[test]
    fn a_pickup_flashes_the_view_and_fades_by_a_tenth_a_frame() {
        let mut world = test_world();
        let key = player_in(&mut world);
        world.client_mut(key).unwrap().bonus_alpha = 0.25;
        world.calc_blend(key);
        assert_eq!(world.client(key).unwrap().ps.blend, [0.85, 0.7, 0.3, 0.25]);
        assert!((world.client(key).unwrap().bonus_alpha - 0.15).abs() < 1e-6);
        world.calc_blend(key);
        world.calc_blend(key);
        assert_eq!(
            world.client(key).unwrap().bonus_alpha,
            0.0,
            "not below zero"
        );
    }

    // ---- P_DamageFeedback ----

    fn hurt_player() -> (
        GameWorld,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        let key = player_in(&mut world);
        world.level.time = 10.0;
        (world, key, gi)
    }

    fn hit(
        world: &mut GameWorld,
        key: EntityKey,
        blood: i32,
        armor: i32,
        parmor: i32,
        knockback: i32,
    ) {
        let client = world.client_mut(key).unwrap();
        client.damage_blood = blood;
        client.damage_armor = armor;
        client.damage_parmor = parmor;
        client.damage_knockback = knockback;
    }

    #[test]
    fn no_damage_this_frame_clears_the_flashes_and_does_nothing_else() {
        let (mut world, key, gi) = hurt_player();
        world.client_mut(key).unwrap().ps.stats[STAT_FLASHES] = 3;
        world.damage_feedback(key);
        let client = world.client(key).unwrap();
        assert_eq!(
            (client.ps.stats[STAT_FLASHES], client.damage_alpha),
            (0, 0.0)
        );
        assert!(gi.sound_calls.borrow().is_empty());
    }

    #[test]
    fn blood_and_armor_flash_their_status_numbers() {
        let (mut world, key, _) = hurt_player();
        hit(&mut world, key, 5, 5, 0, 0);
        world.damage_feedback(key);
        assert_eq!(world.client(key).unwrap().ps.stats[STAT_FLASHES], 3);
        hit(&mut world, key, 0, 5, 0, 0);
        world.client_mut(key).unwrap().invincible_framenum = 200;
        world.damage_feedback(key);
        assert_eq!(
            world.client(key).unwrap().ps.stats[STAT_FLASHES],
            0,
            "armor not hit"
        );
    }

    #[test]
    fn a_hit_plays_a_pain_sound_by_health_at_most_every_0_7_seconds() {
        for (health, level) in [(10, 25), (30, 50), (60, 75), (90, 100)] {
            let (mut world, key, gi) = hurt_player();
            world.entities.get_mut(key).unwrap().game.health = health;
            hit(&mut world, key, 5, 0, 0, 0);
            world.damage_feedback(key);
            let names = gi.sounds.borrow().clone();
            let played: Vec<String> = gi
                .sound_calls
                .borrow()
                .iter()
                .map(|c| names[c.2 as usize - 1].clone())
                .collect();
            assert_eq!(played.len(), 1);
            let want = [format!("*pain{level}_1.wav"), format!("*pain{level}_2.wav")];
            assert!(want.contains(&played[0]), "{played:?}");
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                10.7
            );
            hit(&mut world, key, 5, 0, 0, 0);
            world.damage_feedback(key);
            assert_eq!(gi.sound_calls.borrow().len(), 1, "debounced");
        }
    }

    #[test]
    fn god_mode_and_invulnerability_hurt_silently() {
        let (mut world, key, gi) = hurt_player();
        world.entities.get_mut(key).unwrap().game.flags |= crate::constants::EntityFlags::GODMODE;
        hit(&mut world, key, 5, 0, 0, 0);
        world.damage_feedback(key);
        assert!(gi.sound_calls.borrow().is_empty());
    }

    #[test]
    fn the_damage_tint_grows_with_damage_and_mixes_by_what_absorbed_it() {
        let (mut world, key, _) = hurt_player();
        hit(&mut world, key, 2, 0, 0, 0);
        world.damage_feedback(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.damage_alpha, 0.2, "at least 0.2");
        assert_eq!(client.damage_blend, [1.0, 0.0, 0.0]);

        hit(&mut world, key, 50, 25, 25, 0);
        world.damage_feedback(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.damage_alpha, 0.6, "at most 0.6");
        // Half blood (red), a quarter armor (white), a quarter power (green).
        assert_eq!(client.damage_blend, [0.75, 0.5, 0.25]);
        assert_eq!(
            (
                client.damage_blood,
                client.damage_armor,
                client.damage_parmor
            ),
            (0, 0, 0),
            "totals cleared"
        );
    }

    #[test]
    fn knockback_kicks_the_view_away_from_where_the_hit_came_from() {
        let (mut world, key, _) = hurt_player();
        world.client_mut(key).unwrap().v_angle = Vec3f::ZERO;
        let origin = world.entities.get(key).unwrap().state.origin;
        // From straight ahead (+x): pitch kicks back, no roll.
        world.client_mut(key).unwrap().damage_from = origin + Vec3f::new(100.0, 0.0, 0.0);
        hit(&mut world, key, 20, 0, 0, 10);
        world.damage_feedback(key);
        let client = world.client(key).unwrap();
        // kick = 10 * 100 / 100 = 10, at least count/2 = 10.
        assert!(
            (client.v_dmg_pitch + 3.0).abs() < 1e-4,
            "{}",
            client.v_dmg_pitch
        );
        assert!(client.v_dmg_roll.abs() < 1e-4);
        assert_eq!(client.v_dmg_time, 10.0 + DAMAGE_TIME);

        // From the right (-y): roll.
        world.client_mut(key).unwrap().damage_from = origin + Vec3f::new(0.0, -100.0, 0.0);
        hit(&mut world, key, 20, 0, 0, 500);
        world.damage_feedback(key);
        assert!(
            (world.client(key).unwrap().v_dmg_roll - 15.0).abs() < 1e-4,
            "capped at 50"
        );
    }

    #[test]
    fn no_knockback_means_no_view_kick() {
        let (mut world, key, _) = hurt_player();
        hit(&mut world, key, 20, 0, 0, 0);
        world.damage_feedback(key);
        assert_eq!(world.client(key).unwrap().v_dmg_time, 0.0);
    }

    #[test]
    fn a_hit_starts_the_pain_animation() {
        let (mut world, key, _) = hurt_player();
        hit(&mut world, key, 5, 0, 0, 0);
        world.damage_feedback(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.anim_priority, crate::constants::ANIM_PAIN);
        assert!([53, 57, 61].contains(&world.entities.get(key).unwrap().state.frame));
    }

    #[test]
    fn the_death_sound_waits_for_the_end_of_the_frame() {
        let (mut world, key, gi) = hurt_player();
        world.client_mut(key).unwrap().death_sound = world.gi.sound_index("*death2.wav");
        world.damage_feedback(key);
        let death = world.gi.sound_index("*death2.wav");
        assert!(gi.sound_calls.borrow().iter().any(|c| c.2 == death));
        assert_eq!(world.client(key).unwrap().death_sound, 0);
    }

    #[test]
    fn the_damage_tint_fades_six_hundredths_a_frame() {
        let (mut world, key, _) = hurt_player();
        let client = world.client_mut(key).unwrap();
        client.damage_alpha = 0.3;
        client.damage_blend = [1.0, 0.0, 0.0];
        world.calc_blend(key);
        assert_eq!(world.client(key).unwrap().ps.blend, [1.0, 0.0, 0.0, 0.3]);
        assert!((world.client(key).unwrap().damage_alpha - 0.24).abs() < 1e-6);
    }

    // ---- SV_CalcGunOffset ----

    fn gun_world() -> (GameWorld, EntityKey) {
        let mut world = test_world();
        let key = player_in(&mut world);
        (world, key)
    }

    fn run_gun(world: &mut GameWorld, key: EntityKey) -> (Vec3f, Vec3f) {
        let bob = world.calc_bob(key);
        world.calc_gun_offset(key, &bob);
        let ps = &world.client(key).unwrap().ps;
        (ps.gunangles, ps.gunoffset)
    }

    fn close(a: Vec3f, b: Vec3f) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_still_player_holds_the_gun_level() {
        let (mut world, key) = gun_world();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::ZERO;
        assert_eq!(run_gun(&mut world, key), (Vec3f::ZERO, Vec3f::ZERO));
    }

    #[test]
    fn turning_lags_the_gun_behind_the_view() {
        let (mut world, key) = gun_world();
        let client = world.client_mut(key).unwrap();
        client.oldviewangles = Vec3f::new(0.0, 10.0, 0.0);
        client.ps.viewangles = Vec3f::ZERO;
        let (angles, _) = run_gun(&mut world, key);
        assert!(close(angles, Vec3f::new(0.0, 2.0, 1.0)), "{angles:?}");

        // Clamped to 45 degrees and wrapped across 0/360.
        let client = world.client_mut(key).unwrap();
        client.oldviewangles = Vec3f::new(100.0, 350.0, 0.0);
        client.ps.viewangles = Vec3f::new(0.0, 10.0, 0.0);
        let (angles, _) = run_gun(&mut world, key);
        assert!(close(angles, Vec3f::new(9.0, -4.0, -2.0)), "{angles:?}");
    }

    #[test]
    fn running_on_the_ground_sways_the_gun_with_the_bob() {
        let (mut world, key) = gun_world();
        let ground = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.velocity = Vec3f::new(300.0, 0.0, 0.0);
        ent.game.ground_entity = Some(ground);
        let (angles, _) = run_gun(&mut world, key);
        // bobmove 0.25 above 210 u/s: bobtime 0.25, |sin(pi/4)|.
        let s = (0.25f32 * std::f32::consts::PI).sin() * 300.0;
        assert!(
            close(angles, Vec3f::new(s * 0.005, s * 0.01, s * 0.005)),
            "{angles:?}"
        );
        assert_eq!(world.client(key).unwrap().bobtime, 0.25);

        // An odd bob cycle swings yaw and roll the other way.
        world.client_mut(key).unwrap().bobtime = 1.0;
        let (angles, _) = run_gun(&mut world, key);
        assert!(
            angles.y < 0.0 && angles.z < 0.0 && angles.x > 0.0,
            "{angles:?}"
        );
    }

    #[test]
    fn the_bob_waits_in_the_air_resets_when_still_and_quickens_ducked() {
        let (mut world, key) = gun_world();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(300.0, 0.0, 0.0);
        world.client_mut(key).unwrap().bobtime = 0.5;
        world.calc_bob(key);
        assert_eq!(world.client(key).unwrap().bobtime, 0.5, "airborne");

        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(2.0, 0.0, 0.0);
        world.calc_bob(key);
        assert_eq!(world.client(key).unwrap().bobtime, 0.0, "still");

        let ground = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.velocity = Vec3f::new(150.0, 0.0, 0.0);
        ent.game.ground_entity = Some(ground);
        world.client_mut(key).unwrap().ps.pmove.pm_flags |= q2_common::pmove::PMF_DUCKED;
        let bob = world.calc_bob(key);
        assert_eq!(world.client(key).unwrap().bobtime, 0.125);
        assert_eq!(bob.cycle, 0, "0.125 * 4 = 0.5");
        assert!((bob.fracsin - 1.0).abs() < 1e-5, "|sin(pi/2)|");
    }

    #[test]
    fn the_frame_ends_by_remembering_the_view_angles() {
        let (mut world, key) = gun_world();
        world.client_mut(key).unwrap().ps.viewangles = Vec3f::new(5.0, 6.0, 0.0);
        world.client_end_server_frames();
        assert_eq!(
            world.client(key).unwrap().oldviewangles,
            Vec3f::new(5.0, 6.0, 0.0)
        );
    }

    // ---- P_WorldEffects ----

    fn swimmer() -> (
        GameWorld,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        // The world (edict 0) is what hurts the player, as in C.
        world.entities.spawn_numbered(0).unwrap();
        let key = player_in(&mut world);
        world.level.time = 20.0;
        world.client_mut(key).unwrap().air_finished = 32.0;
        (world, key, gi)
    }

    fn wade(world: &mut GameWorld, key: EntityKey, level: i32, watertype: i32) {
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.waterlevel = level;
        ent.game.watertype = watertype;
    }

    fn heard(world: &GameWorld, gi: &crate::world::MockGameImport, sound: &str) -> bool {
        let index = world.gi.sound_index(sound);
        gi.sound_calls.borrow().iter().any(|c| c.2 == index)
    }

    fn health(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().game.health
    }

    #[test]
    fn stepping_into_and_out_of_water_splashes_and_flags_the_player() {
        let (mut world, key, gi) = swimmer();
        wade(&mut world, key, 1, CONTENTS_WATER);
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/watr_in.wav"));
        let ent = world.entities.get(key).unwrap();
        assert!(ent
            .game
            .flags
            .contains(crate::constants::EntityFlags::INWATER));
        assert_eq!(ent.game.damage_debounce_time, 19.0);

        wade(&mut world, key, 0, 0);
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/watr_out.wav"));
        assert!(!world
            .entities
            .get(key)
            .unwrap()
            .game
            .flags
            .contains(crate::constants::EntityFlags::INWATER));
    }

    #[test]
    fn stepping_into_lava_sounds_like_lava() {
        let (mut world, key, gi) = swimmer();
        wade(&mut world, key, 1, CONTENTS_LAVA);
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/lava_in.wav"));
    }

    #[test]
    fn going_under_and_surfacing_gasp_by_how_long_the_air_lasted() {
        let (mut world, key, gi) = swimmer();
        wade(&mut world, key, 3, CONTENTS_WATER);
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/watr_un.wav"));
        // Surfacing within a second of going under: just breaks surface.
        world.client_mut(key).unwrap().air_finished = 30.5;
        wade(&mut world, key, 2, CONTENTS_WATER);
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/gasp2.wav"));

        let (mut world, key, gi) = swimmer();
        wade(&mut world, key, 3, CONTENTS_WATER);
        world.world_effects(key);
        world.client_mut(key).unwrap().air_finished = 19.0;
        wade(&mut world, key, 1, CONTENTS_WATER);
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/gasp1.wav"), "out of air");
    }

    #[test]
    fn out_of_air_the_player_drowns_harder_each_second_up_to_15() {
        let (mut world, key, gi) = swimmer();
        world.world_effects(key); // a breath on land: dmg 2
        wade(&mut world, key, 3, CONTENTS_WATER);
        world.world_effects(key); // under: air keeps its 32 s
        world.client_mut(key).unwrap().air_finished = 19.0;
        world.world_effects(key);
        assert_eq!(health(&world, key), 96, "2 + 2");
        assert_eq!(world.means_of_death, crate::constants::MeansOfDeath::Water);
        assert!(heard(&world, &gi, "*gurp1.wav") || heard(&world, &gi, "*gurp2.wav"));
        world.world_effects(key);
        assert_eq!(health(&world, key), 96, "once a second");
        // Past next_drown_time (strictly later, as in C) each step.
        for _ in 0..8 {
            world.level.time += 1.5;
            world.world_effects(key);
        }
        assert_eq!(world.entities.get(key).unwrap().game.dmg, 15);

        world.entities.get_mut(key).unwrap().game.health = 10;
        world.level.time += 1.5;
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/drown1.wav"), "the last gurp");
    }

    #[test]
    fn breathing_air_resets_the_air_and_the_drowning_damage() {
        let (mut world, key, _) = swimmer();
        world.entities.get_mut(key).unwrap().game.dmg = 9;
        world.world_effects(key);
        assert_eq!(world.client(key).unwrap().air_finished, 32.0);
        assert_eq!(world.entities.get(key).unwrap().game.dmg, 2);
    }

    #[test]
    fn a_rebreather_gives_air_and_breathes_every_25_frames() {
        let (mut world, key, gi) = swimmer();
        world.client_mut(key).unwrap().breather_framenum = 150;
        wade(&mut world, key, 3, CONTENTS_WATER);
        world.world_effects(key);
        assert_eq!(world.client(key).unwrap().air_finished, 30.0);
        assert!(heard(&world, &gi, "player/u_breath1.wav"));
        world.level.framenum = 101;
        world.world_effects(key);
        assert!(!heard(&world, &gi, "player/u_breath2.wav"));
        world.level.framenum = 125;
        world.world_effects(key);
        assert!(heard(&world, &gi, "player/u_breath2.wav"), "alternates");
        assert_eq!(health(&world, key), 100);
    }

    #[test]
    fn lava_burns_three_per_level_and_a_third_with_the_envirosuit() {
        let (mut world, key, gi) = swimmer();
        wade(&mut world, key, 2, CONTENTS_LAVA);
        world.world_effects(key);
        assert_eq!(health(&world, key), 94);
        assert!(heard(&world, &gi, "player/burn1.wav") || heard(&world, &gi, "player/burn2.wav"));
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            21.0
        );

        let (mut world, key, _) = swimmer();
        world.client_mut(key).unwrap().enviro_framenum = 200;
        wade(&mut world, key, 2, CONTENTS_LAVA);
        world.world_effects(key);
        assert_eq!(health(&world, key), 98);
        assert_eq!(world.means_of_death, crate::constants::MeansOfDeath::Lava);
    }

    #[test]
    fn slime_hurts_one_per_level_unless_the_envirosuit_is_on() {
        let (mut world, key, _) = swimmer();
        wade(&mut world, key, 3, CONTENTS_SLIME);
        world.world_effects(key);
        assert_eq!(health(&world, key), 97);
        assert_eq!(world.means_of_death, crate::constants::MeansOfDeath::Slime);

        let (mut world, key, _) = swimmer();
        world.client_mut(key).unwrap().enviro_framenum = 200;
        wade(&mut world, key, 3, CONTENTS_SLIME);
        world.world_effects(key);
        assert_eq!(health(&world, key), 100);
    }

    #[test]
    fn a_player_spawns_with_twelve_seconds_of_air() {
        let mut world = test_world();
        world.level.time = 7.0;
        let key = player_in(&mut world);
        assert_eq!(world.client(key).unwrap().air_finished, 19.0);
    }

    #[test]
    fn noclip_needs_no_air() {
        let (mut world, key, gi) = swimmer();
        world.entities.get_mut(key).unwrap().game.movetype = crate::constants::MoveType::Noclip;
        wade(&mut world, key, 3, CONTENTS_SLIME);
        world.world_effects(key);
        assert_eq!(world.client(key).unwrap().air_finished, 32.0);
        assert_eq!(health(&world, key), 100);
        assert!(gi.sound_calls.borrow().is_empty());
    }

    #[test]
    fn a_dead_player_gets_no_kick_angles() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        world.entities.get_mut(key).unwrap().game.deadflag = crate::constants::DeadFlag::Dead;
        world.client_mut(key).unwrap().kick_angles = Vec3f::new(-1.0, 0.0, 0.0);
        world.update_player_view(key);
        assert_eq!(world.client(key).unwrap().ps.kick_angles, Vec3f::ZERO);
    }

    #[test]
    fn view_offset_default_height() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key); // viewheight 22
        world.entities.get_mut(key).unwrap().velocity = Vec3f::ZERO;

        world.update_player_view(key);

        let offset = world.client(key).unwrap().ps.viewoffset;
        assert!((offset.z - 22.0).abs() < 1.0);
    }

    #[test]
    fn view_offset_follows_the_entity_viewheight_like_sv_calcviewoffset() {
        // pmove sets viewheight to -2 when ducked; the eye must drop with it.
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().game.viewheight = -2.0;

        world.update_player_view(key);

        assert_eq!(world.client(key).unwrap().ps.viewoffset.z, -2.0);
    }

    #[test]
    fn view_bob_increases_with_speed() {
        let bob_at = |speed: f32| {
            let mut world = test_world();
            world.level.time = 1.0;
            let key = world.connect_client(1, "\\name\\P").unwrap();
            world.begin_client(key);
            let ent = world.entities.get_mut(key).unwrap();
            ent.velocity = Vec3f::new(speed, 0.0, 0.0);
            ent.game.ground_entity = Some(key);
            world.update_player_view(key);
            let ent = world.entities.get(key).unwrap();
            world.client(key).unwrap().ps.viewoffset.z - ent.game.viewheight
        };
        let (still, running) = (bob_at(0.0), bob_at(300.0));
        assert_eq!(still, 0.0);
        assert_ne!(running, still);
    }
}
