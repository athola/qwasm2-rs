//! The player's guns and the noises monsters hear. C ref:
//! `player/weapon.c`.

use q2_common::pmove::PMF_DUCKED;
use q2_shared::constants::{
    BUTTON_ATTACK, EF_BLASTER, MZ_BLASTER, MZ_HYPERBLASTER, MZ_SILENCED, SVF_NOCLIENT,
};
use q2_shared::protocol::SvcOp;
use q2_shared::types::{Multicast, Vec3f};

use crate::constants::*;
use crate::entity::{EntityKey, WeaponState};
use crate::world::GameWorld;

// Player model frames the weapon code starts (C `player.h`).
const FRAME_ATTACK1: i32 = 46;
const FRAME_ATTACK8: i32 = 53;
const FRAME_PAIN301: i32 = 62;
const FRAME_PAIN304: i32 = 65;
const FRAME_CRATTAK1: i32 = 160;
const FRAME_CRATTAK9: i32 = 168;
const FRAME_CRPAIN1: i32 = 169;
const FRAME_CRPAIN4: i32 = 172;

/// `spawnflags` of a player's own noise marker (C `PLAYER_NOISE_SELF`).
pub const PLAYER_NOISE_SELF: i32 = 0;
/// `spawnflags` of a player's impact noise marker (C `PLAYER_NOISE_IMPACT`).
pub const PLAYER_NOISE_IMPACT: i32 = 1;

/// What made a player noise (C `PNOISE_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerNoise {
    /// Jumping, splashing, gasping (C `PNOISE_SELF`).
    SelfNoise,
    /// Firing a weapon (C `PNOISE_WEAPON`).
    Weapon,
    /// Where a shot landed (C `PNOISE_IMPACT`).
    Impact,
}

impl GameWorld {
    /// Each player owns two invisible markers that monsters hear: one for
    /// its own and weapon noises (`level.sound_entity`), one for impacts
    /// (`level.sound2_entity`, heard only by monsters without an enemy).
    /// A noise moves the marker to `at`, at most once every 4 frames per
    /// kind. Deathmatch monsters hear nothing; a notarget player makes no
    /// sound, and a silenced shot uses up a silencer charge instead.
    /// C ref: `PlayerNoise` (player/weapon.c). Markers are made on first
    /// use as in id's original; yquake2's `PlayerNoise_Verify` scan for
    /// orphans from a loaded save waits on save/load (#15).
    pub fn player_noise(&mut self, who: EntityKey, at: Vec3f, kind: PlayerNoise) {
        if self.client(who).is_none() {
            return;
        }
        if kind == PlayerNoise::Weapon {
            if let Some(client) = self.client_mut(who).filter(|c| c.silencer_shots > 0) {
                client.silencer_shots -= 1;
                return;
            }
        }
        if self.game.deathmatch
            || self
                .entities
                .get(who)
                .is_some_and(|e| e.game.flags.contains(EntityFlags::NOTARGET))
        {
            return;
        }
        self.player_noise_verify(who);

        let framenum = self.level.framenum;
        let noise = if kind == PlayerNoise::Impact {
            if framenum <= self.level.sound2_entity_framenum + 3 {
                return;
            }
            let Some(noise) = self.entities.get(who).and_then(|e| e.game.mynoise2) else {
                return;
            };
            self.level.sound2_entity = Some(noise);
            self.level.sound2_entity_framenum = framenum;
            noise
        } else {
            if framenum <= self.level.sound_entity_framenum + 3 {
                return;
            }
            let Some(noise) = self.entities.get(who).and_then(|e| e.game.mynoise) else {
                return;
            };
            self.level.sound_entity = Some(noise);
            self.level.sound_entity_framenum = framenum;
            noise
        };
        // ai.rs reads C's `last_sound_time` from the framenums set above.
        if let Some(ent) = self.entities.get_mut(noise) {
            ent.state.origin = at;
        }
        self.link_entity(noise);
    }

    /// Make the player's markers that are missing or freed.
    /// C ref: `PlayerNoise_Verify` / `PlayerNoise_Spawn`.
    fn player_noise_verify(&mut self, who: EntityKey) {
        let Some(me) = self.entities.get(who) else {
            return;
        };
        let live = |k: Option<EntityKey>| k.filter(|&k| self.entities.is_live(k));
        let (mut n1, mut n2) = (live(me.game.mynoise), live(me.game.mynoise2));
        if n1.is_none() {
            n1 = self.player_noise_spawn(who, PLAYER_NOISE_SELF);
        }
        if n2.is_none() {
            n2 = self.player_noise_spawn(who, PLAYER_NOISE_IMPACT);
        }
        if let Some(me) = self.entities.get_mut(who) {
            me.game.mynoise = n1;
            me.game.mynoise2 = n2;
        }
    }

    /// Run the current weapon for a frame; a dead player's weapon goes
    /// away first. C ref: `Think_Weapon` (player/weapon.c), from
    /// `ClientThink` on a fresh attack press or `ClientBeginServerFrame`.
    pub fn think_weapon(&mut self, key: EntityKey) {
        let dead = self.entities.get(key).is_none_or(|e| e.game.health < 1);
        if dead {
            if let Some(client) = self.client_mut(key) {
                client.newweapon = None;
            }
            self.change_weapon(key);
        }
        let think = self
            .client(key)
            .and_then(|c| c.pers.weapon)
            .and_then(|w| self.items.get(w))
            .and_then(|item| item.weaponthink);
        if let Some(think) = think {
            think(self, key);
        }
    }

    /// Make `newweapon` current once the old one is down: its view model,
    /// its ammo, and the player's raise animation. With no weapon (dead)
    /// the gun model goes. C ref: `ChangeWeapon` (player/weapon.c); the
    /// held-grenade throw waits on the hand grenade.
    pub fn change_weapon(&mut self, key: EntityKey) {
        let Some(client) = self.client_mut(key) else {
            return;
        };
        client.pers.last_weapon = client.pers.weapon;
        client.pers.weapon = client.newweapon.take();
        let weapon = client.pers.weapon;

        let item = weapon.and_then(|w| self.items.get(w));
        let weapmodel = item.map_or(0, |item| item.tag);
        let ammo = item.map(|item| item.ammo).filter(|a| !a.is_empty());
        let ammo_index = ammo.and_then(|a| self.find_item_by_name(a)).unwrap_or(0);
        let view_model = item.map(|item| item.view_model);
        let gunindex = view_model.map(|m| self.gi.model_index(m));
        let number = self.number_of(key).unwrap_or(1);

        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        // The skin carries the client slot and the weapon for C's vwep.
        if ent.state.modelindex == 255 {
            ent.state.skinnum = (number as i32 - 1) | ((weapmodel & 0xff) << 8);
        }
        client.ammo_index = ammo_index;
        let Some(gunindex) = gunindex else {
            client.ps.gunindex = 0; // dead
            return;
        };
        client.weaponstate = WeaponState::Activating;
        client.ps.gunframe = 0;
        client.ps.gunindex = gunindex;
        client.anim_priority = ANIM_PAIN;
        if client.ps.pmove.pm_flags & PMF_DUCKED != 0 {
            ent.state.frame = FRAME_CRPAIN1;
            client.anim_end = FRAME_CRPAIN4;
        } else {
            ent.state.frame = FRAME_PAIN301;
            client.anim_end = FRAME_PAIN304;
        }
    }

    /// The best weapon the player has ammo for, else the blaster. C ref:
    /// `NoAmmoWeaponChange` (player/weapon.c).
    pub fn no_ammo_weapon_change(&mut self, key: EntityKey) {
        let has = |world: &GameWorld, name: &str, at_least: i32| {
            let Some(client) = world.client(key) else {
                return false;
            };
            world
                .find_item_by_name(name)
                .is_some_and(|i| client.pers.inventory[i] >= at_least)
        };
        let choice = [
            ("Slugs", 1, "Railgun"),
            ("Cells", 1, "HyperBlaster"),
            ("Bullets", 1, "Chaingun"),
            ("Bullets", 1, "Machinegun"),
            ("Shells", 2, "Super Shotgun"),
            ("Shells", 1, "Shotgun"),
        ]
        .into_iter()
        .find(|&(ammo, count, weapon)| has(self, ammo, count) && has(self, weapon, 1))
        .map_or("Blaster", |(_, _, weapon)| weapon);
        let newweapon = self.find_item_by_name(choice);
        if let Some(client) = self.client_mut(key) {
            client.newweapon = newweapon;
        }
    }

    /// Quad damage multiplier and muzzle flash silencer bit for a shot
    /// now (C `is_quad`, `is_silenced`, set by `Think_Weapon`).
    fn shot_modifiers(&self, key: EntityKey) -> (bool, i32) {
        self.client(key).map_or((false, 0), |c| {
            let silenced = if c.silencer_shots > 0 { MZ_SILENCED } else { 0 };
            (c.quad_framenum > self.level.framenum, silenced)
        })
    }

    /// Tell clients in the PVS the player fired (C `svc_muzzleflash`).
    fn player_muzzle_flash(&self, key: EntityKey, flash: i32) {
        let Some(number) = self.number_of(key) else {
            return;
        };
        let origin = self
            .entities
            .get(key)
            .map_or(Vec3f::ZERO, |e| e.state.origin);
        self.gi.write_byte(SvcOp::MuzzleFlash as i32);
        self.gi.write_short(number as i32);
        self.gi.write_byte(flash);
        self.gi.multicast(origin, Multicast::PVS);
    }

    /// Where a shot leaves the player: `offset` forward/right/up from the
    /// origin along `v_angle`, right-handed. Returns (start, forward,
    /// right). C ref: `P_ProjectSource` with `aimfix` 0.
    fn shot_source(&self, key: EntityKey, offset: [f32; 3]) -> (Vec3f, Vec3f, Vec3f) {
        let (origin, v_angle) = match (self.entities.get(key), self.client(key)) {
            (Some(ent), Some(client)) => (ent.state.origin, client.v_angle),
            _ => (Vec3f::ZERO, Vec3f::ZERO),
        };
        let (forward, right, _) = crate::physics::angle_vectors(v_angle);
        let start = crate::monster::flash::project_source(origin, offset, forward, right);
        (start, forward, right)
    }

    /// C ref: `PlayerNoise_Spawn` (player/weapon.c).
    fn player_noise_spawn(&mut self, who: EntityKey, spawnflags: i32) -> Option<EntityKey> {
        let noise = self.spawn()?;
        let ent = self.entities.get_mut(noise)?;
        ent.game.classname = "player_noise".to_string();
        ent.game.spawnflags = spawnflags;
        ent.mins = Vec3f::splat(-8.0);
        ent.maxs = Vec3f::splat(8.0);
        ent.owner = Some(who);
        ent.svflags = SVF_NOCLIENT;
        Some(noise)
    }
}

/// Play the backwards lowering animation. C ref: `Change_Weap_Animation`.
fn change_weap_animation(world: &mut GameWorld, key: EntityKey) {
    let Some((ent, client)) = world.entity_and_client_mut(key) else {
        return;
    };
    client.anim_priority = ANIM_REVERSE;
    if client.ps.pmove.pm_flags & PMF_DUCKED != 0 {
        ent.state.frame = FRAME_CRPAIN4 + 1;
        client.anim_end = FRAME_CRPAIN1;
    } else {
        ent.state.frame = FRAME_PAIN304 + 1;
        client.anim_end = FRAME_PAIN301;
    }
}

/// Fire callback of `weapon_generic` (C `void (*fire)(edict_t *)`).
pub type WeaponFireFn = fn(&mut GameWorld, EntityKey);

/// The gun's frame cycle shared by every weapon: raise through
/// `activate_last`, fire on `fire_frames` through `fire_last`, idle to
/// `idle_last` (pausing at random on `pause_frames`), lower through
/// `deactivate_last` when `newweapon` is set. C ref: `Weapon_Generic`
/// (player/weapon.c) with `g_swap_speed` 1, its default.
#[allow(clippy::too_many_arguments)]
pub fn weapon_generic(
    world: &mut GameWorld,
    key: EntityKey,
    activate_last: i32,
    fire_last: i32,
    idle_last: i32,
    deactivate_last: i32,
    pause_frames: &[i32],
    fire_frames: &[i32],
    fire: WeaponFireFn,
) {
    let fire_first = activate_last + 1;
    let idle_first = fire_last + 1;
    let deactivate_first = idle_last + 1;

    // VWep animations screw up corpses.
    let skip = world
        .entities
        .get(key)
        .is_none_or(|e| e.game.deadflag != DeadFlag::No || e.state.modelindex != 255);
    if skip {
        return;
    }
    let Some(client) = world.client_mut(key) else {
        return;
    };

    match client.weaponstate {
        WeaponState::Dropping => {
            if client.ps.gunframe == deactivate_last {
                world.change_weapon(key);
                return;
            }
            let at_four = deactivate_last - client.ps.gunframe == 4;
            client.ps.gunframe += 1;
            if at_four {
                change_weap_animation(world, key);
            }
            return;
        }
        WeaponState::Activating => {
            if client.ps.gunframe == activate_last {
                client.weaponstate = WeaponState::Ready;
                client.ps.gunframe = idle_first;
            } else {
                client.ps.gunframe += 1;
            }
            return;
        }
        _ => {}
    }

    if client.newweapon.is_some() && client.weaponstate != WeaponState::Firing {
        client.weaponstate = WeaponState::Dropping;
        client.ps.gunframe = deactivate_first;
        if deactivate_last - deactivate_first < 4 {
            change_weap_animation(world, key);
        }
        return;
    }

    if client.weaponstate == WeaponState::Ready {
        if (client.latched_buttons | client.buttons) & BUTTON_ATTACK == 0 {
            if client.ps.gunframe == idle_last {
                client.ps.gunframe = idle_first;
                return;
            }
            if pause_frames.contains(&client.ps.gunframe) && world.random_u32() & 15 != 0 {
                return;
            }
            if let Some(client) = world.client_mut(key) {
                client.ps.gunframe += 1;
            }
            return;
        }
        client.latched_buttons &= !BUTTON_ATTACK;
        let per_shot = client
            .pers
            .weapon
            .and_then(|w| world.items.get(w))
            .map_or(0, |item| item.quantity);
        let now = world.level.time;
        let Some((ent, client)) = world.entity_and_client_mut(key) else {
            return;
        };
        if client.ammo_index == 0 || client.pers.inventory[client.ammo_index] >= per_shot {
            client.ps.gunframe = fire_first;
            client.weaponstate = WeaponState::Firing;
            client.anim_priority = ANIM_ATTACK;
            if client.ps.pmove.pm_flags & PMF_DUCKED != 0 {
                ent.state.frame = FRAME_CRATTAK1 - 1;
                client.anim_end = FRAME_CRATTAK9;
            } else {
                ent.state.frame = FRAME_ATTACK1 - 1;
                client.anim_end = FRAME_ATTACK8;
            }
        } else {
            if now >= ent.game.pain_debounce_time {
                ent.game.pain_debounce_time = now + 1.0;
                let number = world.number_of(key);
                let noammo = world.gi.sound_index("weapons/noammo.wav");
                world
                    .gi
                    .sound(number, CHAN_VOICE, noammo, 1.0, ATTN_NORM, 0.0);
            }
            world.no_ammo_weapon_change(key);
        }
    }

    let Some(client) = world.client(key) else {
        return;
    };
    if client.weaponstate == WeaponState::Firing {
        if fire_frames.contains(&client.ps.gunframe) {
            if client.quad_framenum > world.level.framenum {
                let number = world.number_of(key);
                let damage3 = world.gi.sound_index("items/damage3.wav");
                world
                    .gi
                    .sound(number, CHAN_ITEM, damage3, 1.0, ATTN_NORM, 0.0);
            }
            fire(world, key);
        } else if let Some(client) = world.client_mut(key) {
            client.ps.gunframe += 1;
        }
        if let Some(client) = world.client_mut(key) {
            if client.ps.gunframe == idle_first + 1 {
                client.weaponstate = WeaponState::Ready;
            }
        }
    }
}

/// A blaster or hyperblaster bolt from `g_offset` past the usual muzzle,
/// with kick and muzzle flash. C ref: `Blaster_Fire` (player/weapon.c);
/// `fire_blaster` raises the weapon noise.
pub fn blaster_fire(
    world: &mut GameWorld,
    key: EntityKey,
    g_offset: Vec3f,
    damage: i32,
    hyper: bool,
    effect: u32,
) {
    let (quad, silenced) = world.shot_modifiers(key);
    let damage = if quad { damage * 4 } else { damage };
    let viewheight = world.entities.get(key).map_or(0.0, |e| e.game.viewheight);
    let offset = [
        24.0 + g_offset.x,
        8.0 + g_offset.y,
        viewheight - 8.0 + g_offset.z,
    ];
    let (start, forward, _) = world.shot_source(key, offset);
    if let Some(client) = world.client_mut(key) {
        client.kick_origin = forward * -2.0;
        client.kick_angles.x = -1.0;
    }
    world.fire_blaster(key, start, forward, damage, 1000, effect, hyper);
    let flash = if hyper { MZ_HYPERBLASTER } else { MZ_BLASTER };
    world.player_muzzle_flash(key, flash | silenced);
}

/// C ref: `Weapon_Blaster_Fire` (player/weapon.c).
pub fn weapon_blaster_fire(world: &mut GameWorld, key: EntityKey) {
    let damage = if world.game.deathmatch { 15 } else { 10 };
    blaster_fire(world, key, Vec3f::ZERO, damage, false, EF_BLASTER);
    if let Some(client) = world.client_mut(key) {
        client.ps.gunframe += 1;
    }
}

/// C ref: `Weapon_Blaster` (player/weapon.c).
pub fn weapon_blaster(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        4,
        8,
        52,
        55,
        &[19, 32],
        &[5],
        weapon_blaster_fire,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::AiFlags;
    use crate::entity::{ClientData, MonsterInfo};
    use crate::world::test_world;

    fn player(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn_test_client(ClientData::default());
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        key
    }

    /// A frame late enough that C's "+ 3 frames" throttle has passed.
    fn noisy_world() -> GameWorld {
        let mut world = test_world();
        world.level.framenum = 10;
        world.level.sound_entity_framenum = -10;
        world.level.sound2_entity_framenum = -10;
        world
    }

    #[test]
    fn player_noise_self_moves_the_players_marker_there_and_makes_it_the_sound_entity() {
        let mut world = noisy_world();
        let who = player(&mut world);
        let at = Vec3f::new(10.0, 20.0, 30.0);

        world.player_noise(who, at, PlayerNoise::SelfNoise);

        let noise = world.level.sound_entity.expect("a noise");
        assert_eq!(world.level.sound_entity_framenum, 10);
        assert_eq!(world.entities.get(who).unwrap().game.mynoise, Some(noise));
        let ent = world.entities.get(noise).unwrap();
        assert_eq!(ent.game.classname, "player_noise");
        assert_eq!(ent.game.spawnflags, PLAYER_NOISE_SELF);
        assert_eq!(ent.owner, Some(who));
        assert_eq!(ent.svflags, SVF_NOCLIENT);
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::splat(-8.0), Vec3f::splat(8.0))
        );
        assert_eq!(ent.state.origin, at);
        assert!(ent.linkcount > 0, "linked");
        assert_eq!(world.level.sound2_entity, None);
    }

    #[test]
    fn player_noise_weapon_uses_the_same_marker_as_self() {
        let mut world = noisy_world();
        let who = player(&mut world);

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Weapon);

        let mynoise = world.entities.get(who).unwrap().game.mynoise;
        assert_eq!(world.level.sound_entity, mynoise);
        assert!(mynoise.is_some());
    }

    #[test]
    fn player_noise_impact_moves_the_second_marker_and_makes_it_sound2() {
        let mut world = noisy_world();
        let who = player(&mut world);
        let at = Vec3f::new(-5.0, 0.0, 0.0);

        world.player_noise(who, at, PlayerNoise::Impact);

        let noise = world.level.sound2_entity.expect("an impact noise");
        assert_eq!(world.level.sound2_entity_framenum, 10);
        assert_eq!(world.level.sound_entity, None);
        let me = world.entities.get(who).unwrap();
        assert_eq!(me.game.mynoise2, Some(noise));
        assert_ne!(me.game.mynoise, Some(noise));
        let ent = world.entities.get(noise).unwrap();
        assert_eq!(ent.game.spawnflags, PLAYER_NOISE_IMPACT);
        assert_eq!(ent.state.origin, at);
    }

    #[test]
    fn player_noise_waits_three_frames_after_the_last_noise() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.level.sound_entity_framenum = 7;
        world.level.sound2_entity_framenum = 7;

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);
        assert_eq!(world.level.sound_entity, None);
        assert_eq!(world.level.sound2_entity, None);

        world.level.framenum = 11;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);
        assert_eq!(world.level.sound_entity_framenum, 11);
        assert_eq!(world.level.sound2_entity_framenum, 11);
    }

    #[test]
    fn player_noise_keeps_the_players_markers_from_noise_to_noise() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        let first = world.level.sound_entity;
        let count = world.entities.count();

        world.level.framenum = 20;
        world.player_noise(who, Vec3f::X, PlayerNoise::SelfNoise);

        assert_eq!(world.level.sound_entity, first);
        assert_eq!(world.entities.count(), count);
    }

    #[test]
    fn player_noise_replaces_a_freed_marker() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        let first = world.level.sound_entity.unwrap();
        world.free_entity(first);

        world.level.framenum = 20;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);

        let second = world.level.sound_entity.unwrap();
        assert_ne!(second, first);
        assert!(world.entities.is_live(second));
    }

    #[test]
    fn player_noise_is_silent_in_deathmatch() {
        let mut world = noisy_world();
        world.game.deathmatch = true;
        let who = player(&mut world);

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);

        assert_eq!(world.level.sound_entity, None);
        assert_eq!(world.entities.get(who).unwrap().game.mynoise, None);
    }

    #[test]
    fn player_noise_is_silent_for_a_notarget_player() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.entities.get_mut(who).unwrap().game.flags |= EntityFlags::NOTARGET;

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);

        assert_eq!(world.level.sound2_entity, None);
    }

    #[test]
    fn player_noise_ignores_entities_without_a_client() {
        let mut world = noisy_world();
        let monster = world.spawn().unwrap();

        world.player_noise(monster, Vec3f::ZERO, PlayerNoise::SelfNoise);

        assert_eq!(world.level.sound_entity, None);
        assert_eq!(world.entities.count(), 1);
    }

    #[test]
    fn player_noise_weapon_uses_up_a_silenced_shot_instead() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.client_mut(who).unwrap().silencer_shots = 1;

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Weapon);
        assert_eq!(world.client(who).unwrap().silencer_shots, 0);
        assert_eq!(world.level.sound_entity, None);

        // The silencer muffles only weapon fire.
        world.client_mut(who).unwrap().silencer_shots = 1;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        assert_eq!(world.client(who).unwrap().silencer_shots, 1);
        assert!(world.level.sound_entity.is_some());
    }

    /// A monster with no enemy standing 300 units from the player.
    fn listening_monster(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::new(300.0, 0.0, 24.0);
        ent.svflags = q2_shared::constants::SVF_MONSTER;
        ent.game.health = 20;
        ent.monsterinfo = Some(Box::new(MonsterInfo::default()));
        key
    }

    #[test]
    fn a_player_firing_makes_a_noise_a_monster_in_hearing_range_hunts() {
        let mut world = noisy_world();
        let who = player(&mut world);
        // Behind a wall the monster cannot see through: it can only hear.
        world.entities.get_mut(who).unwrap().game.light_level = 0;
        let monster = listening_monster(&mut world);
        assert!(!crate::ai::find_target(&mut world, monster), "unseen");

        world.fire_blaster(
            who,
            Vec3f::new(0.0, 0.0, 30.0),
            Vec3f::Y,
            15,
            1000,
            0,
            false,
        );
        let noise = world.level.sound_entity.expect("the shot is heard");
        assert_eq!(world.entities.get(noise).unwrap().owner, Some(who));

        assert!(crate::ai::find_target(&mut world, monster));
        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(noise));
        assert!(ent
            .monsterinfo
            .as_ref()
            .unwrap()
            .aiflags
            .contains(AiFlags::SOUND_TARGET));
    }

    // ---- Weapon state machine (C Think_Weapon / Weapon_Generic) ----

    use crate::entity::WeaponState;
    use crate::world::{test_world_recording, MockGameImport};
    use q2_common::pmove::PMF_DUCKED;
    use q2_shared::constants::{BUTTON_ATTACK, MZ_BLASTER};
    use q2_shared::protocol::SvcOp;
    use q2_shared::types::Multicast;
    use std::rc::Rc;

    /// A player put in the game like C PutClientInServer: blaster raised.
    fn armed() -> (GameWorld, EntityKey, Rc<MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        (world, key, gi)
    }

    fn think_n(world: &mut GameWorld, key: EntityKey, n: usize) {
        for _ in 0..n {
            world.think_weapon(key);
        }
    }

    fn bolts(world: &GameWorld) -> usize {
        world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.game.classname == "bolt")
            .count()
    }

    #[test]
    fn the_blaster_row_carries_c_view_model_and_weapon_think() {
        let mut world = test_world();
        world.init_items();
        let blaster = world.find_item_by_name("Blaster").unwrap();
        let item = &world.items[blaster];
        assert_eq!(item.view_model, "models/weapons/v_blast/tris.md2");
        assert!(item.weaponthink.is_some());
    }

    #[test]
    fn put_client_in_server_raises_the_current_weapon() {
        let (world, key, _) = armed();
        let client = world.client(key).unwrap();
        let blaster = world.find_item_by_name("Blaster");
        assert_eq!(client.pers.weapon, blaster);
        assert_eq!(client.newweapon, None);
        assert_eq!(client.weaponstate, WeaponState::Activating);
        assert_eq!(client.ps.gunframe, 0);
        assert_eq!(
            client.ps.gunindex,
            world.gi.model_index("models/weapons/v_blast/tris.md2")
        );
        assert_eq!(client.ammo_index, 0, "the blaster uses no ammo");
    }

    #[test]
    fn the_blaster_activates_through_frame_4_then_idles_from_frame_9() {
        let (mut world, key, _) = armed();
        think_n(&mut world, key, 4);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 4);
        assert_eq!(
            world.client(key).unwrap().weaponstate,
            WeaponState::Activating
        );
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.weaponstate, WeaponState::Ready);
        assert_eq!(client.ps.gunframe, 9);
    }

    fn ready() -> (GameWorld, EntityKey, Rc<MockGameImport>) {
        let (mut world, key, gi) = armed();
        think_n(&mut world, key, 5);
        (world, key, gi)
    }

    #[test]
    fn idle_frames_run_to_52_and_wrap_to_9() {
        let (mut world, key, _) = ready();
        world.client_mut(key).unwrap().ps.gunframe = 51;
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 52);
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 9);
    }

    #[test]
    fn idle_pause_frames_hold_most_frames_but_not_forever() {
        let (mut world, key, _) = ready();
        world.client_mut(key).unwrap().ps.gunframe = 19;
        let mut held = 0;
        for _ in 0..200 {
            world.think_weapon(key);
            if world.client(key).unwrap().ps.gunframe != 19 {
                break;
            }
            held += 1;
        }
        assert!(held > 0, "C holds unless randk() & 15 is 0");
        assert_eq!(world.client(key).unwrap().ps.gunframe, 20, "left the pause");
    }

    #[test]
    fn a_press_fires_one_bolt_on_frame_5_with_muzzle_flash_and_kick() {
        let (mut world, key, mock) = ready();
        world.client_mut(key).unwrap().latched_buttons = BUTTON_ATTACK;
        mock.writes.borrow_mut().clear();
        // READY turns to FIRING on frame 5, which fires in the same think.
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.weaponstate, WeaponState::Firing);
        assert_eq!(client.latched_buttons, 0, "the press is used up");
        assert_eq!(client.anim_priority, ANIM_ATTACK);
        assert_eq!(world.entities.get(key).unwrap().state.frame, 45);
        assert_eq!(client.anim_end, 53);
        assert_eq!(bolts(&world), 1);
        let number = world.number_of(key).unwrap() as i32;
        assert_eq!(
            *mock.writes.borrow(),
            vec![SvcOp::MuzzleFlash as i32, number, MZ_BLASTER]
        );
        assert_eq!(mock.multicasts.borrow().last().unwrap().1, Multicast::PVS);
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.gunframe, 6);
        assert_eq!(client.kick_angles.x, -1.0);
        assert!(
            client.kick_origin.length() > 1.9,
            "{:?}",
            client.kick_origin
        );

        think_n(&mut world, key, 3);
        assert_eq!(world.client(key).unwrap().weaponstate, WeaponState::Firing);
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.gunframe, 10);
        assert_eq!(client.weaponstate, WeaponState::Ready);
        assert_eq!(bolts(&world), 1, "one bolt per press");
    }

    #[test]
    fn a_held_button_keeps_firing_from_ready() {
        let (mut world, key, _) = ready();
        world.client_mut(key).unwrap().buttons = BUTTON_ATTACK;
        think_n(&mut world, key, 7);
        assert_eq!(bolts(&world), 2);
    }

    #[test]
    fn a_silenced_shot_flags_the_muzzle_flash() {
        let (mut world, key, mock) = ready();
        let client = world.client_mut(key).unwrap();
        client.latched_buttons = BUTTON_ATTACK;
        client.silencer_shots = 5;
        think_n(&mut world, key, 2);
        assert_eq!(
            mock.writes.borrow().last().copied(),
            Some(MZ_BLASTER | q2_shared::constants::MZ_SILENCED)
        );
    }

    #[test]
    fn quad_damage_plays_its_sound_on_each_fire_frame() {
        let (mut world, key, mock) = ready();
        let framenum = world.level.framenum;
        let client = world.client_mut(key).unwrap();
        client.latched_buttons = BUTTON_ATTACK;
        client.quad_framenum = framenum + 100;
        think_n(&mut world, key, 2);
        let damage3 = world.gi.sound_index("items/damage3.wav");
        assert!(mock
            .sound_calls
            .borrow()
            .iter()
            .any(|call| call.2 == damage3));
        let bolt = world
            .entities
            .iter()
            .find(|(_, e)| e.game.classname == "bolt")
            .unwrap()
            .1;
        assert_eq!(bolt.game.dmg, 40, "10 x4");
    }

    #[test]
    fn a_new_weapon_lowers_the_current_one_then_raises_the_new_one() {
        let (mut world, key, _) = ready();
        let blaster = world.find_item_by_name("Blaster").unwrap();
        let shotgun = world.find_item_by_name("Shotgun").unwrap();
        world.client_mut(key).unwrap().newweapon = Some(shotgun);
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.weaponstate, WeaponState::Dropping);
        assert_eq!(client.ps.gunframe, 53);
        // 55 - 53 < 4: C plays the lowering animation at once, backwards.
        assert_eq!(client.anim_priority, ANIM_REVERSE);
        assert_eq!(world.entities.get(key).unwrap().state.frame, 66);
        assert_eq!(client.anim_end, 62);

        think_n(&mut world, key, 2);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 55);
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.pers.weapon, Some(shotgun));
        assert_eq!(client.pers.last_weapon, Some(blaster));
        assert_eq!(client.newweapon, None);
        assert_eq!(client.weaponstate, WeaponState::Activating);
        assert_eq!(
            client.ps.gunindex,
            world.gi.model_index("models/weapons/v_shotg/tris.md2")
        );
        assert_eq!(
            client.ammo_index,
            world.find_item_by_name("Shells").unwrap()
        );
        assert_eq!(client.anim_priority, ANIM_PAIN);
    }

    #[test]
    fn a_new_weapon_waits_for_the_shot_to_finish() {
        let (mut world, key, _) = ready();
        let shotgun = world.find_item_by_name("Shotgun").unwrap();
        world.client_mut(key).unwrap().latched_buttons = BUTTON_ATTACK;
        world.think_weapon(key);
        world.client_mut(key).unwrap().newweapon = Some(shotgun);
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().weaponstate, WeaponState::Firing);
        assert_eq!(bolts(&world), 1);
    }

    #[test]
    fn a_dead_player_puts_the_weapon_away() {
        let (mut world, key, _) = ready();
        world.entities.get_mut(key).unwrap().game.health = 0;
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.pers.weapon, None);
        assert_eq!(client.ps.gunindex, 0);
    }

    #[test]
    fn a_gibbed_or_dead_player_runs_no_weapon_frames() {
        let (mut world, key, _) = ready();
        world.entities.get_mut(key).unwrap().state.modelindex = 0;
        world.client_mut(key).unwrap().latched_buttons = BUTTON_ATTACK;
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 9);

        let (mut world, key, _) = ready();
        world.entities.get_mut(key).unwrap().game.deadflag = DeadFlag::Dead;
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 9);
    }

    /// Records the frames `weapon_generic` fires on.
    fn record_fire(world: &mut GameWorld, key: EntityKey) {
        let client = world.client_mut(key).unwrap();
        client.ps.gunframe += 1;
        client.anim_end = -1;
    }

    #[test]
    fn without_enough_ammo_weapon_generic_clicks_and_picks_another_weapon() {
        let (mut world, key, mock) = ready();
        let shotgun = world.find_item_by_name("Shotgun").unwrap();
        let shells = world.find_item_by_name("Shells").unwrap();
        let client = world.client_mut(key).unwrap();
        client.pers.weapon = Some(shotgun);
        client.ammo_index = shells;
        client.latched_buttons = BUTTON_ATTACK;
        weapon_generic(&mut world, key, 7, 18, 36, 39, &[], &[8], record_fire);
        let client = world.client(key).unwrap();
        assert_eq!(client.weaponstate, WeaponState::Ready, "no shot");
        assert_eq!(client.newweapon, world.find_item_by_name("Blaster"));
        let noammo = world.gi.sound_index("weapons/noammo.wav");
        assert!(mock.sound_calls.borrow().iter().any(|c| c.2 == noammo));
        // The click repeats at most once a second.
        assert!(world.entities.get(key).unwrap().game.pain_debounce_time > world.level.time);

        let client = world.client_mut(key).unwrap();
        client.pers.inventory[shells] = 1;
        client.latched_buttons = BUTTON_ATTACK;
        client.newweapon = None;
        weapon_generic(&mut world, key, 7, 18, 36, 39, &[], &[8], record_fire);
        assert_eq!(world.client(key).unwrap().weaponstate, WeaponState::Firing);
    }

    #[test]
    fn crouched_firing_plays_the_crouch_attack_frames() {
        let (mut world, key, _) = ready();
        let client = world.client_mut(key).unwrap();
        client.ps.pmove.pm_flags |= PMF_DUCKED;
        client.latched_buttons = BUTTON_ATTACK;
        world.think_weapon(key);
        assert_eq!(world.entities.get(key).unwrap().state.frame, 159);
        assert_eq!(world.client(key).unwrap().anim_end, 168);
    }
}
