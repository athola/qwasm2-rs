//! Player view — camera offset, view bob, damage kicks, fall effects.
//! C ref: `player/view.c` (1,426 lines)

use q2_shared::types::*;

use q2_shared::constants::{
    CONTENTS_LAVA, CONTENTS_SLIME, CONTENTS_SOLID, CONTENTS_WATER, EF_COLOR_SHELL, EF_PENT,
    EF_POWERSCREEN, EF_QUAD, RDF_UNDERWATER, RF_IR_VISIBLE, RF_SHELL_BLUE, RF_SHELL_GREEN,
    RF_SHELL_RED,
};

use crate::constants::{
    EntityFlags, ATTN_NORM, CHAN_ITEM, DAMAGE_TIME, POWER_ARMOR_SCREEN, POWER_ARMOR_SHIELD,
};
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
    /// pickup flash. C ref: `SV_CalcBlend` (player/view.c); the damage
    /// flash waits on `P_DamageFeedback`.
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
        if client.bonus_alpha > 0.0 {
            add_blend([0.85, 0.7, 0.3], client.bonus_alpha, blend);
        }
        client.bonus_alpha = (client.bonus_alpha - 0.1).max(0.0);
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
