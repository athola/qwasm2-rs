//! The player's guns and the noises monsters hear. C ref:
//! `player/weapon.c`.

use q2_common::pmove::PMF_DUCKED;
use q2_shared::constants::{
    BUTTON_ATTACK, EF_BLASTER, EF_HYPERBLASTER, MZ_BFG, MZ_BLASTER, MZ_CHAINGUN1, MZ_GRENADE,
    MZ_HYPERBLASTER, MZ_MACHINEGUN, MZ_RAILGUN, MZ_ROCKET, MZ_SHOTGUN, MZ_SILENCED, MZ_SSHOTGUN,
    PRINT_HIGH, SVF_NOCLIENT,
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
const FRAME_WAVE01: i32 = 112;
const FRAME_WAVE08: i32 = 119;
const FRAME_CRATTAK1: i32 = 160;
const FRAME_CRATTAK3: i32 = 162;
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
        let time = self.level.time;
        if let Some(ent) = self.entities.get_mut(noise) {
            ent.state.origin = at;
            ent.game.last_sound_time = time;
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
    /// the gun model goes. C ref: `ChangeWeapon` (player/weapon.c).
    pub fn change_weapon(&mut self, key: EntityKey) {
        let now = self.level.time;
        let Some(client) = self.client_mut(key) else {
            return;
        };
        // Switching away from a cooked grenade throws it.
        if client.grenade_time != 0.0 {
            client.grenade_time = now;
            client.weapon_sound = 0;
            weapon_grenade_fire(self, key, false);
            if let Some(client) = self.client_mut(key) {
                client.grenade_time = 0.0;
            }
        }
        let Some(client) = self.client_mut(key) else {
            return;
        };
        client.pers.last_weapon = client.pers.weapon;
        client.pers.weapon = client.newweapon.take();
        client.machinegun_shots = 0;
        let weapon = client.pers.weapon;

        let item = weapon.and_then(|w| self.items.get(w));
        let weapmodel = item.map_or(0, |item| item.weapmodel);
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
    /// origin along `v_angle`, from the player's hand. Returns (start,
    /// forward, right). C ref: `P_ProjectSource` with `aimfix` 0.
    fn shot_source(&self, key: EntityKey, offset: [f32; 3]) -> (Vec3f, Vec3f, Vec3f) {
        let v_angle = self.client(key).map_or(Vec3f::ZERO, |c| c.v_angle);
        let (forward, right, _) = crate::physics::angle_vectors(v_angle);
        (
            self.player_project_source(key, offset, forward, right),
            forward,
            right,
        )
    }

    /// C `P_ProjectSource`: `offset` from the player's origin, its right
    /// part mirrored for a left hand and dropped for a centre one.
    fn player_project_source(
        &self,
        key: EntityKey,
        offset: [f32; 3],
        forward: Vec3f,
        right: Vec3f,
    ) -> Vec3f {
        let origin = self
            .entities
            .get(key)
            .map_or(Vec3f::ZERO, |e| e.state.origin);
        let mut offset = offset;
        match self.client(key).map_or(RIGHT_HANDED, |c| c.pers.hand) {
            LEFT_HANDED => offset[1] = -offset[1],
            CENTER_HANDED => offset[1] = 0.0,
            _ => {}
        }
        crate::monster::flash::project_source(origin, offset, forward, right)
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

/// Make `item` the weapon to switch to, if there is ammo for a shot (C
/// `g_select_empty` 0). C ref: `Use_Weapon` (player/weapon.c).
pub fn use_weapon(world: &mut GameWorld, key: EntityKey, item: usize) {
    let Some(client) = world.client(key) else {
        return;
    };
    // See if we're already using it.
    if client.pers.weapon == Some(item) {
        return;
    }
    let Some(weapon) = world.items.get(item) else {
        return;
    };
    let ammo = (!weapon.ammo.is_empty() && !weapon.flags.contains(ItemFlags::AMMO))
        .then(|| world.find_item_by_name(weapon.ammo))
        .flatten();
    if let Some(ammo) = ammo {
        let have = client.pers.inventory[ammo];
        let (ammo_name, weapon_name) = (world.items[ammo].pickup_name, weapon.pickup_name);
        let problem = if have == 0 {
            Some(format!("No {ammo_name} for {weapon_name}.\n"))
        } else if have < weapon.quantity {
            Some(format!("Not enough {ammo_name} for {weapon_name}.\n"))
        } else {
            None
        };
        if let Some(problem) = problem {
            world.gi.cprintf(world.number_of(key), PRINT_HIGH, &problem);
            return;
        }
    }
    // Change to this weapon when down.
    if let Some(client) = world.client_mut(key) {
        client.newweapon = Some(item);
    }
}

/// Throw a spare weapon away; not the only copy of the one in hand, and
/// not at all with weapons stay. C ref: `Drop_Weapon` (player/weapon.c).
pub fn drop_weapon(world: &mut GameWorld, key: EntityKey, item: usize) {
    if world.game.dmflags & DF_WEAPONS_STAY != 0 {
        return;
    }
    let Some(client) = world.client(key) else {
        return;
    };
    let in_hand = client.pers.weapon == Some(item) || client.newweapon == Some(item);
    if in_hand && client.pers.inventory[item] == 1 {
        world.gi.cprintf(
            world.number_of(key),
            PRINT_HIGH,
            "Can't drop current weapon\n",
        );
        return;
    }
    world.drop_item(key, item);
    if let Some(client) = world.client_mut(key) {
        client.pers.inventory[item] -= 1;
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
        let Some(client) = world.client_mut(key) else {
            return;
        };
        if client.ammo_index == 0 || client.pers.inventory[client.ammo_index] >= per_shot {
            client.ps.gunframe = fire_first;
            client.weaponstate = WeaponState::Firing;
            attack_animation(world, key, 1);
        } else {
            click_and_switch(world, key);
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
/// with kick, muzzle flash and weapon noise. C ref: `Blaster_Fire`
/// (player/weapon.c).
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
    world.player_noise(key, start, PlayerNoise::Weapon);
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

/// Spend `count` of the current weapon's ammo, unless `dmflags` has
/// infinite ammo.
fn use_ammo(world: &mut GameWorld, key: EntityKey, count: i32) {
    if world.game.dmflags & DF_INFINITE_AMMO != 0 {
        return;
    }
    if let Some(client) = world.client_mut(key) {
        let index = client.ammo_index;
        client.pers.inventory[index] -= count;
    }
}

/// The current weapon's ammo count.
fn ammo_left(world: &GameWorld, key: EntityKey) -> i32 {
    world
        .client(key)
        .map_or(0, |c| c.pers.inventory[c.ammo_index])
}

/// The "click" of an empty gun, at most once a second, then the best
/// loaded weapon. C: the `weapons/noammo.wav` block repeated in each
/// weapon.
fn click_and_switch(world: &mut GameWorld, key: EntityKey) {
    let now = world.level.time;
    let click = world.entities.get_mut(key).is_some_and(|ent| {
        let due = now >= ent.game.pain_debounce_time;
        if due {
            ent.game.pain_debounce_time = now + 1.0;
        }
        due
    });
    if click {
        let number = world.number_of(key);
        let noammo = world.gi.sound_index("weapons/noammo.wav");
        world
            .gi
            .sound(number, CHAN_VOICE, noammo, 1.0, ATTN_NORM, 0.0);
    }
    world.no_ammo_weapon_change(key);
}

/// Start the attack animation `back` frames before its first frame.
fn attack_animation(world: &mut GameWorld, key: EntityKey, back: i32) {
    let Some((ent, client)) = world.entity_and_client_mut(key) else {
        return;
    };
    client.anim_priority = ANIM_ATTACK;
    if client.ps.pmove.pm_flags & PMF_DUCKED != 0 {
        ent.state.frame = FRAME_CRATTAK1 - back;
        client.anim_end = FRAME_CRATTAK9;
    } else {
        ent.state.frame = FRAME_ATTACK1 - back;
        client.anim_end = FRAME_ATTACK8;
    }
}

fn step_gunframe(world: &mut GameWorld, key: EntityKey) {
    if let Some(client) = world.client_mut(key) {
        client.ps.gunframe += 1;
    }
}

fn viewheight(world: &GameWorld, key: EntityKey) -> f32 {
    world.entities.get(key).map_or(0.0, |e| e.game.viewheight)
}

/// Kick the view straight back by `back` and up by `pitch`.
fn kick(world: &mut GameWorld, key: EntityKey, forward: Vec3f, back: f32, pitch: f32) {
    if let Some(client) = world.client_mut(key) {
        client.kick_origin = forward * -back;
        client.kick_angles.x = pitch;
    }
}

/// C ref: `weapon_shotgun_fire` (player/weapon.c).
pub fn weapon_shotgun_fire(world: &mut GameWorld, key: EntityKey) {
    if world.client(key).is_some_and(|c| c.ps.gunframe == 9) {
        step_gunframe(world, key);
        return;
    }
    let (quad, silenced) = world.shot_modifiers(key);
    let (damage, kick_power) = if quad { (16, 32) } else { (4, 8) };
    let (start, forward, _) = world.shot_source(key, [0.0, 8.0, viewheight(world, key) - 8.0]);
    kick(world, key, forward, 2.0, -2.0);
    let count = if world.game.deathmatch {
        DEFAULT_DEATHMATCH_SHOTGUN_COUNT
    } else {
        DEFAULT_SHOTGUN_COUNT
    };
    world.fire_shotgun(
        key,
        start,
        forward,
        damage,
        kick_power,
        500,
        500,
        count,
        MeansOfDeath::Shotgun,
    );
    world.player_muzzle_flash(key, MZ_SHOTGUN | silenced);
    world.player_noise(key, start, PlayerNoise::Weapon);
    step_gunframe(world, key);
    use_ammo(world, key, 1);
}

/// C ref: `Weapon_Shotgun` (player/weapon.c).
pub fn weapon_shotgun(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        7,
        18,
        36,
        39,
        &[22, 28, 34],
        &[8, 9],
        weapon_shotgun_fire,
    );
}

/// Two half loads 5 degrees either side of the aim. C ref:
/// `weapon_supershotgun_fire` (player/weapon.c).
pub fn weapon_supershotgun_fire(world: &mut GameWorld, key: EntityKey) {
    let (quad, silenced) = world.shot_modifiers(key);
    let (damage, kick_power) = if quad { (24, 48) } else { (6, 12) };
    let (start, forward, _) = world.shot_source(key, [0.0, 8.0, viewheight(world, key) - 8.0]);
    kick(world, key, forward, 2.0, -2.0);
    let v_angle = world.client(key).map_or(Vec3f::ZERO, |c| c.v_angle);
    for yaw in [-5.0, 5.0] {
        let (forward, _, _) =
            crate::physics::angle_vectors(Vec3f::new(v_angle.x, v_angle.y + yaw, v_angle.z));
        world.fire_shotgun(
            key,
            start,
            forward,
            damage,
            kick_power,
            DEFAULT_SHOTGUN_HSPREAD,
            DEFAULT_SHOTGUN_VSPREAD,
            DEFAULT_SSHOTGUN_COUNT / 2,
            MeansOfDeath::SShotgun,
        );
    }
    world.player_muzzle_flash(key, MZ_SSHOTGUN | silenced);
    world.player_noise(key, start, PlayerNoise::Weapon);
    step_gunframe(world, key);
    use_ammo(world, key, 2);
}

/// C ref: `Weapon_SuperShotgun` (player/weapon.c).
pub fn weapon_supershotgun(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        6,
        17,
        57,
        61,
        &[29, 42, 57],
        &[7],
        weapon_supershotgun_fire,
    );
}

/// One bullet a frame while held, the aim climbing with each shot
/// (single player). C ref: `Machinegun_Fire` (player/weapon.c), with
/// `g_machinegun_norecoil` 0.
pub fn machinegun_fire(world: &mut GameWorld, key: EntityKey) {
    let Some(client) = world.client_mut(key) else {
        return;
    };
    if client.buttons & BUTTON_ATTACK == 0 {
        client.machinegun_shots = 0;
        client.ps.gunframe += 1;
        return;
    }
    client.ps.gunframe = if client.ps.gunframe == 5 { 4 } else { 5 };
    if ammo_left(world, key) < 1 {
        if let Some(client) = world.client_mut(key) {
            client.ps.gunframe = 6;
        }
        click_and_switch(world, key);
        return;
    }
    let (quad, silenced) = world.shot_modifiers(key);
    let (damage, kick_power) = if quad { (32, 8) } else { (8, 2) };
    let jitter: [f32; 5] = std::array::from_fn(|_| world.crandom());
    let deathmatch = world.game.deathmatch;
    let Some(client) = world.client_mut(key) else {
        return;
    };
    for i in 1..3 {
        client.kick_origin[i] = jitter[2 * i - 2] * 0.35;
        client.kick_angles[i] = jitter[2 * i - 1] * 0.7;
    }
    client.kick_origin.x = jitter[4] * 0.35;
    client.kick_angles.x = client.machinegun_shots as f32 * -1.5;
    // Raise the gun as it is firing.
    if !deathmatch {
        client.machinegun_shots = (client.machinegun_shots + 1).min(9);
    }
    let angles = client.v_angle + client.kick_angles;
    let (forward, right, _) = crate::physics::angle_vectors(angles);
    let offset = [0.0, 8.0, viewheight(world, key) - 8.0];
    let start = world.player_project_source(key, offset, forward, right);
    world.fire_bullet(
        key,
        start,
        forward,
        damage,
        kick_power,
        DEFAULT_BULLET_HSPREAD,
        DEFAULT_BULLET_VSPREAD,
        MeansOfDeath::Machinegun,
    );
    world.player_muzzle_flash(key, MZ_MACHINEGUN | silenced);
    world.player_noise(key, start, PlayerNoise::Weapon);
    use_ammo(world, key, 1);
    let back = (world.random() + 0.25) as i32;
    attack_animation(world, key, back);
}

/// C ref: `Weapon_Machinegun` (player/weapon.c).
pub fn weapon_machinegun(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        3,
        5,
        45,
        49,
        &[23, 45],
        &[4, 5],
        machinegun_fire,
    );
}

/// Spins up from one to three bullets a frame while held and winds down
/// on release. C ref: `Chaingun_Fire` (player/weapon.c).
pub fn chaingun_fire(world: &mut GameWorld, key: EntityKey) {
    let damage = if world.game.deathmatch { 6 } else { 8 };
    let number = world.number_of(key);
    let ammo = ammo_left(world, key);
    let Some(client) = world.client(key) else {
        return;
    };
    let held = client.buttons & BUTTON_ATTACK != 0;
    if client.ps.gunframe == 5 {
        let spin_up = world.gi.sound_index("weapons/chngnu1a.wav");
        world
            .gi
            .sound(number, CHAN_AUTO, spin_up, 1.0, ATTN_IDLE, 0.0);
    }
    let Some(client) = world.client_mut(key) else {
        return;
    };
    if client.ps.gunframe == 14 && !held {
        client.ps.gunframe = 32;
        client.weapon_sound = 0;
        return;
    } else if client.ps.gunframe == 21 && held && ammo > 0 {
        client.ps.gunframe = 15;
    } else {
        client.ps.gunframe += 1;
    }
    let gunframe = client.ps.gunframe;
    if gunframe == 22 {
        client.weapon_sound = 0;
        let spin_down = world.gi.sound_index("weapons/chngnd1a.wav");
        world
            .gi
            .sound(number, CHAN_AUTO, spin_down, 1.0, ATTN_IDLE, 0.0);
    } else {
        let spin = world.gi.sound_index("weapons/chngnl1a.wav");
        if let Some(client) = world.client_mut(key) {
            client.weapon_sound = spin;
        }
    }
    attack_animation(world, key, gunframe & 1);

    let shots = match gunframe {
        ..=9 => 1,
        10..=14 if held => 2,
        10..=14 => 1,
        _ => 3,
    }
    .min(ammo);
    if shots == 0 {
        click_and_switch(world, key);
        return;
    }
    let (quad, silenced) = world.shot_modifiers(key);
    let (damage, kick_power) = if quad { (damage * 4, 8) } else { (damage, 2) };
    let jitter: [f32; 6] = std::array::from_fn(|_| world.crandom());
    if let Some(client) = world.client_mut(key) {
        for i in 0..3 {
            client.kick_origin[i] = jitter[2 * i] * 0.35;
            client.kick_angles[i] = jitter[2 * i + 1] * 0.7;
        }
    }
    let viewheight = viewheight(world, key);
    // C makes the noise at the last bullet's muzzle (`shots` is at least 1).
    let mut start = Vec3f::ZERO;
    for _ in 0..shots {
        let r = 7.0 + world.crandom() * 4.0;
        let u = world.crandom() * 4.0;
        let forward;
        (start, forward, _) = world.shot_source(key, [0.0, r, u + viewheight - 8.0]);
        world.fire_bullet(
            key,
            start,
            forward,
            damage,
            kick_power,
            DEFAULT_BULLET_HSPREAD,
            DEFAULT_BULLET_VSPREAD,
            MeansOfDeath::Chaingun,
        );
    }
    world.player_muzzle_flash(key, (MZ_CHAINGUN1 + shots - 1) | silenced);
    world.player_noise(key, start, PlayerNoise::Weapon);
    use_ammo(world, key, shots);
}

/// C ref: `Weapon_Chaingun` (player/weapon.c).
pub fn weapon_chaingun(world: &mut GameWorld, key: EntityKey) {
    let fire_frames: [i32; 17] = std::array::from_fn(|i| 5 + i as i32);
    weapon_generic(
        world,
        key,
        4,
        31,
        61,
        64,
        &[38, 43, 51, 61],
        &fire_frames,
        chaingun_fire,
    );
}

/// Where the hyperblaster's barrel puts the bolt on `gunframe`. C computes
/// the rotation in double, stores it in a float, then takes double
/// `sin`/`cos` of it. C ref: `Weapon_HyperBlaster_Fire` (player/weapon.c).
fn hyperblaster_offset(gunframe: i32) -> Vec3f {
    let rotation = (f64::from(gunframe - 5) * 2.0 * std::f64::consts::PI / 6.0) as f32;
    let rotation = f64::from(rotation);
    Vec3f::new(
        (-4.0 * rotation.sin()) as f32,
        0.0,
        (4.0 * rotation.cos()) as f32,
    )
}

/// A bolt a frame from a barrel turning through six positions, looping
/// while held and loaded. C ref: `Weapon_HyperBlaster_Fire`.
pub fn weapon_hyperblaster_fire(world: &mut GameWorld, key: EntityKey) {
    let hum = world.gi.sound_index("weapons/hyprbl1a.wav");
    let ammo = ammo_left(world, key);
    let Some(client) = world.client_mut(key) else {
        return;
    };
    client.weapon_sound = hum;
    if client.buttons & BUTTON_ATTACK == 0 {
        client.ps.gunframe += 1;
    } else {
        let gunframe = client.ps.gunframe;
        if ammo == 0 {
            click_and_switch(world, key);
        } else {
            let offset = hyperblaster_offset(gunframe);
            let effect = if gunframe == 6 || gunframe == 9 {
                EF_HYPERBLASTER
            } else {
                0
            };
            let damage = if world.game.deathmatch { 15 } else { 20 };
            blaster_fire(world, key, offset, damage, true, effect);
            use_ammo(world, key, 1);
            attack_animation(world, key, 1);
        }
        let ammo = ammo_left(world, key);
        if let Some(client) = world.client_mut(key) {
            client.ps.gunframe += 1;
            if client.ps.gunframe == 12 && ammo > 0 {
                client.ps.gunframe = 6;
            }
        }
    }
    if world.client(key).is_some_and(|c| c.ps.gunframe == 12) {
        let number = world.number_of(key);
        let wind_down = world.gi.sound_index("weapons/hyprbd1a.wav");
        world
            .gi
            .sound(number, CHAN_AUTO, wind_down, 1.0, ATTN_NORM, 0.0);
        if let Some(client) = world.client_mut(key) {
            client.weapon_sound = 0;
        }
    }
}

/// C ref: `Weapon_HyperBlaster` (player/weapon.c).
pub fn weapon_hyperblaster(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        5,
        20,
        49,
        53,
        &[],
        &[6, 7, 8, 9, 10, 11],
        weapon_hyperblaster_fire,
    );
}

/// C ref: `Weapon_RocketLauncher_Fire` (player/weapon.c).
pub fn weapon_rocketlauncher_fire(world: &mut GameWorld, key: EntityKey) {
    let (quad, silenced) = world.shot_modifiers(key);
    let mut damage = 100 + (world.random() * 20.0) as i32;
    let mut radius_damage = 120;
    if quad {
        damage *= 4;
        radius_damage *= 4;
    }
    let (start, forward, _) = world.shot_source(key, [8.0, 8.0, viewheight(world, key) - 8.0]);
    kick(world, key, forward, 2.0, -1.0);
    world.fire_rocket(key, start, forward, damage, 650, 120.0, radius_damage);
    world.player_muzzle_flash(key, MZ_ROCKET | silenced);
    world.player_noise(key, start, PlayerNoise::Weapon);
    step_gunframe(world, key);
    use_ammo(world, key, 1);
}

/// C ref: `Weapon_RocketLauncher` (player/weapon.c).
pub fn weapon_rocketlauncher(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        4,
        12,
        50,
        54,
        &[25, 33, 42, 50],
        &[5],
        weapon_rocketlauncher_fire,
    );
}

/// C ref: `weapon_grenadelauncher_fire` (player/weapon.c).
pub fn weapon_grenadelauncher_fire(world: &mut GameWorld, key: EntityKey) {
    let (quad, silenced) = world.shot_modifiers(key);
    let damage = if quad { 480 } else { 120 };
    let radius = 120.0 + 40.0; // from the unquadded damage
    let (start, forward, _) = world.shot_source(key, [8.0, 8.0, viewheight(world, key) - 8.0]);
    kick(world, key, forward, 2.0, -1.0);
    world.fire_grenade(key, start, forward, damage, 600, 2.5, radius);
    world.player_muzzle_flash(key, MZ_GRENADE | silenced);
    world.player_noise(key, start, PlayerNoise::Weapon);
    step_gunframe(world, key);
    use_ammo(world, key, 1);
}

/// C ref: `Weapon_GrenadeLauncher` (player/weapon.c).
pub fn weapon_grenadelauncher(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        5,
        16,
        59,
        64,
        &[34, 51, 59],
        &[6],
        weapon_grenadelauncher_fire,
    );
}

/// C ref: `weapon_railgun_fire` (player/weapon.c).
pub fn weapon_railgun_fire(world: &mut GameWorld, key: EntityKey) {
    let (quad, silenced) = world.shot_modifiers(key);
    let (mut damage, mut kick_power) = if world.game.deathmatch {
        (100, 200)
    } else {
        (150, 250)
    };
    if quad {
        damage *= 4;
        kick_power *= 4;
    }
    let (start, forward, _) = world.shot_source(key, [0.0, 7.0, viewheight(world, key) - 8.0]);
    kick(world, key, forward, 3.0, -3.0);
    world.fire_rail(key, start, forward, damage, kick_power);
    world.player_muzzle_flash(key, MZ_RAILGUN | silenced);
    world.player_noise(key, start, PlayerNoise::Weapon);
    step_gunframe(world, key);
    use_ammo(world, key, 1);
}

/// C ref: `Weapon_Railgun` (player/weapon.c).
pub fn weapon_railgun(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(world, key, 3, 18, 56, 61, &[56], &[4], weapon_railgun_fire);
}

/// Frame 9 flashes, frame 17 launches the blast if 50 cells are still
/// there, with a big pitch kick. C ref: `weapon_bfg_fire`.
pub fn weapon_bfg_fire(world: &mut GameWorld, key: EntityKey) {
    let (quad, silenced) = world.shot_modifiers(key);
    if world.client(key).is_some_and(|c| c.ps.gunframe == 9) {
        world.player_muzzle_flash(key, MZ_BFG | silenced);
        step_gunframe(world, key);
        // C passes `start` here before it is ever set (undefined); the
        // noise is made at the muzzle the blast will leave from.
        let start = world
            .shot_source(key, [8.0, 8.0, viewheight(world, key) - 8.0])
            .0;
        world.player_noise(key, start, PlayerNoise::Weapon);
        return;
    }
    // Cells can go down during the windup (power armor hits).
    if ammo_left(world, key) < 50 {
        step_gunframe(world, key);
        return;
    }
    let mut damage = if world.game.deathmatch { 200 } else { 500 };
    if quad {
        damage *= 4;
    }
    let (start, forward, _) = world.shot_source(key, [8.0, 8.0, viewheight(world, key) - 8.0]);
    let roll = world.crandom() * 8.0;
    let now = world.level.time;
    if let Some(client) = world.client_mut(key) {
        client.kick_origin = forward * -2.0;
        // A big pitch kick with an inverse fall.
        client.v_dmg_pitch = -40.0;
        client.v_dmg_roll = roll;
        client.v_dmg_time = now + DAMAGE_TIME;
    }
    world.fire_bfg(key, start, forward, damage, 400, 1000.0);
    step_gunframe(world, key);
    world.player_noise(key, start, PlayerNoise::Weapon);
    use_ammo(world, key, 50);
}

/// C ref: `Weapon_BFG` (player/weapon.c).
pub fn weapon_bfg(world: &mut GameWorld, key: EntityKey) {
    weapon_generic(
        world,
        key,
        8,
        32,
        55,
        58,
        &[39, 45, 50, 55],
        &[9, 17],
        weapon_bfg_fire,
    );
}

const GRENADE_TIMER: f32 = 3.0;
const GRENADE_MINSPEED: f32 = 400.0;
const GRENADE_MAXSPEED: f32 = 800.0;

/// Throw (or, `held`, drop where the player stands) the cooked grenade:
/// the longer it was held, the faster it flies and the sooner it goes
/// off. C ref: `weapon_grenade_fire` (player/weapon.c).
pub fn weapon_grenade_fire(world: &mut GameWorld, key: EntityKey, held: bool) {
    let (quad, _) = world.shot_modifiers(key);
    let mut damage = 125;
    let radius = (damage + 40) as f32;
    if quad {
        damage *= 4;
        let number = world.number_of(key);
        let damage3 = world.gi.sound_index("items/damage3.wav");
        world
            .gi
            .sound(number, CHAN_ITEM, damage3, 1.0, ATTN_NORM, 0.0);
    }
    let (start, forward, _) = world.shot_source(key, [8.0, 8.0, viewheight(world, key) - 8.0]);
    let grenade_time = world.client(key).map_or(0.0, |c| c.grenade_time);
    let timer = grenade_time - world.level.time;
    let speed = GRENADE_MINSPEED
        + (GRENADE_TIMER - timer) * ((GRENADE_MAXSPEED - GRENADE_MINSPEED) / GRENADE_TIMER);
    world.fire_grenade2(
        key,
        start,
        forward,
        damage,
        speed as i32,
        timer,
        radius,
        held,
    );
    use_ammo(world, key, 1);
    let now = world.level.time;
    if let Some(client) = world.client_mut(key) {
        client.grenade_time = now + 1.0;
    }

    // VWep animations screw up corpses.
    let Some((ent, client)) = world.entity_and_client_mut(key) else {
        return;
    };
    if ent.game.deadflag != DeadFlag::No || ent.state.modelindex != 255 || ent.game.health <= 0 {
        return;
    }
    if client.ps.pmove.pm_flags & PMF_DUCKED != 0 {
        client.anim_priority = ANIM_ATTACK;
        ent.state.frame = FRAME_CRATTAK1 - 1;
        client.anim_end = FRAME_CRATTAK3;
    } else {
        client.anim_priority = ANIM_REVERSE;
        ent.state.frame = FRAME_WAVE08;
        client.anim_end = FRAME_WAVE01;
    }
}

/// The hand grenade's own cycle: pull the pin (frames 1-10), wait at 11
/// while held (it goes off in the hand after `GRENADE_TIMER`), throw at
/// 12, recover by 16. C ref: `Weapon_Grenade` (player/weapon.c).
pub fn weapon_grenade(world: &mut GameWorld, key: EntityKey) {
    let ammo = ammo_left(world, key);
    let now = world.level.time;
    let Some(client) = world.client_mut(key) else {
        return;
    };
    if client.newweapon.is_some() && client.weaponstate == WeaponState::Ready {
        world.change_weapon(key);
        return;
    }
    match client.weaponstate {
        WeaponState::Activating => {
            client.weaponstate = WeaponState::Ready;
            client.ps.gunframe = 16;
        }
        WeaponState::Ready => {
            if (client.latched_buttons | client.buttons) & BUTTON_ATTACK != 0 {
                client.latched_buttons &= !BUTTON_ATTACK;
                if ammo > 0 {
                    client.ps.gunframe = 1;
                    client.weaponstate = WeaponState::Firing;
                    client.grenade_time = 0.0;
                } else {
                    click_and_switch(world, key);
                }
                return;
            }
            if [29, 34, 39, 48].contains(&client.ps.gunframe) && world.random_u32() & 15 != 0 {
                return;
            }
            if let Some(client) = world.client_mut(key) {
                client.ps.gunframe += 1;
                if client.ps.gunframe > 48 {
                    client.ps.gunframe = 16;
                }
            }
        }
        WeaponState::Firing => {
            let gunframe = client.ps.gunframe;
            let number = world.number_of(key);
            if gunframe == 5 {
                let pin = world.gi.sound_index("weapons/hgrena1b.wav");
                world
                    .gi
                    .sound(number, CHAN_WEAPON, pin, 1.0, ATTN_NORM, 0.0);
            }
            if world.client(key).is_some_and(|c| c.ps.gunframe == 11) {
                let tick = world.gi.sound_index("weapons/hgrenc1b.wav");
                let Some(client) = world.client_mut(key) else {
                    return;
                };
                if client.grenade_time == 0.0 {
                    client.grenade_time = now + GRENADE_TIMER + 0.2;
                    client.weapon_sound = tick;
                }
                // They waited too long: it goes off in their hand.
                if !client.grenade_blew_up && now >= client.grenade_time {
                    client.weapon_sound = 0;
                    weapon_grenade_fire(world, key, true);
                    if let Some(client) = world.client_mut(key) {
                        client.grenade_blew_up = true;
                    }
                }
                let Some(client) = world.client_mut(key) else {
                    return;
                };
                if client.buttons & BUTTON_ATTACK != 0 {
                    return;
                }
                if client.grenade_blew_up {
                    if now >= client.grenade_time {
                        client.ps.gunframe = 15;
                        client.grenade_blew_up = false;
                    } else {
                        return;
                    }
                }
            }
            if world.client(key).is_some_and(|c| c.ps.gunframe == 12) {
                if let Some(client) = world.client_mut(key) {
                    client.weapon_sound = 0;
                }
                weapon_grenade_fire(world, key, false);
            }
            let Some(client) = world.client_mut(key) else {
                return;
            };
            if client.ps.gunframe == 15 && now < client.grenade_time {
                return;
            }
            client.ps.gunframe += 1;
            if client.ps.gunframe == 16 {
                client.grenade_time = 0.0;
                client.weaponstate = WeaponState::Ready;
            }
        }
        WeaponState::Dropping => {}
    }
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
    fn shots_leave_from_the_players_hand_like_p_projectsource() {
        let mut world = test_world();
        let key = player(&mut world);
        // v_angle 0: forward +x, right -y.
        let start = |world: &mut GameWorld, hand| {
            world.client_mut(key).unwrap().pers.hand = hand;
            world.shot_source(key, [8.0, 8.0, 0.0]).0
        };
        assert_eq!(start(&mut world, RIGHT_HANDED), Vec3f::new(8.0, -8.0, 24.0));
        assert_eq!(start(&mut world, LEFT_HANDED), Vec3f::new(8.0, 8.0, 24.0));
        assert_eq!(start(&mut world, CENTER_HANDED), Vec3f::new(8.0, 0.0, 24.0));
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
    fn player_noise_stamps_its_marker_with_the_time() {
        // C PlayerNoise: noise->last_sound_time = level.time.
        let mut world = noisy_world();
        let who = player(&mut world);
        world.level.time = 1.0;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);
        let noise = world.level.sound2_entity.expect("a noise");
        assert_eq!(world.entities.get(noise).unwrap().game.last_sound_time, 1.0);
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

        weapon_blaster_fire(&mut world, who);
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
    use q2_shared::constants::EF_HYPERBLASTER;
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

    // ---- The other weapons (C Weapon_* / *_Fire) ----

    use q2_shared::constants::{
        MZ_BFG, MZ_CHAINGUN2, MZ_CHAINGUN3, MZ_GRENADE, MZ_MACHINEGUN, MZ_RAILGUN, MZ_ROCKET,
        MZ_SHOTGUN, MZ_SSHOTGUN,
    };

    /// Give `weapon` and `count` of `ammo`, switch to it and wait until it
    /// is up (READY on its first idle frame).
    fn equip(world: &mut GameWorld, key: EntityKey, weapon: &str, ammo: &str, count: i32) {
        let weapon = world.find_item_by_name(weapon).unwrap();
        let ammo = world.find_item_by_name(ammo).unwrap();
        let client = world.client_mut(key).unwrap();
        client.pers.inventory[weapon] = 1;
        client.pers.inventory[ammo] = count;
        client.newweapon = Some(weapon);
        for _ in 0..200 {
            world.think_weapon(key);
            let client = world.client(key).unwrap();
            if client.pers.weapon == Some(weapon) && client.weaponstate == WeaponState::Ready {
                return;
            }
        }
        panic!("weapon never came up");
    }

    fn inventory(world: &GameWorld, key: EntityKey, name: &str) -> i32 {
        let index = world.find_item_by_name(name).unwrap();
        world.client(key).unwrap().pers.inventory[index]
    }

    fn press(world: &mut GameWorld, key: EntityKey) {
        world.client_mut(key).unwrap().latched_buttons = BUTTON_ATTACK;
        world.think_weapon(key);
    }

    fn hold(world: &mut GameWorld, key: EntityKey, held: bool) {
        world.client_mut(key).unwrap().buttons = if held { BUTTON_ATTACK } else { 0 };
    }

    fn last_flash(mock: &MockGameImport) -> Option<i32> {
        mock.writes.borrow().last().copied()
    }

    fn spawned<'a>(world: &'a GameWorld, classname: &str) -> Vec<&'a crate::entity::Entity> {
        world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.game.classname == classname)
            .map(|(_, e)| e)
            .collect()
    }

    #[test]
    fn the_shotgun_fires_once_on_frame_8_for_one_shell() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Shotgun", "Shells", 10);
        press(&mut world, key);
        assert_eq!(last_flash(&mock), Some(MZ_SHOTGUN));
        assert_eq!(inventory(&world, key, "Shells"), 9);
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.gunframe, 9);
        assert_eq!(client.kick_angles.x, -2.0);
        let writes = mock.writes.borrow().len();
        world.think_weapon(key); // frame 9 is a fire frame that only steps
        assert_eq!(world.client(key).unwrap().ps.gunframe, 10);
        assert_eq!(mock.writes.borrow().len(), writes);
        assert_eq!(inventory(&world, key, "Shells"), 9);
    }

    #[test]
    fn the_super_shotgun_needs_and_uses_two_shells() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Super Shotgun", "Shells", 2);
        press(&mut world, key);
        assert_eq!(last_flash(&mock), Some(MZ_SSHOTGUN));
        assert_eq!(inventory(&world, key, "Shells"), 0);

        let (mut world, key, _) = ready();
        equip(&mut world, key, "Super Shotgun", "Shells", 1);
        press(&mut world, key);
        assert_eq!(inventory(&world, key, "Shells"), 1, "no shot");
        assert_eq!(
            world.client(key).unwrap().newweapon,
            world.find_item_by_name("Blaster")
        );
    }

    #[test]
    fn the_machinegun_fires_every_frame_held_and_climbs() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Machinegun", "Bullets", 50);
        hold(&mut world, key, true);
        for _ in 0..3 {
            world.think_weapon(key);
        }
        assert_eq!(inventory(&world, key, "Bullets"), 47);
        assert_eq!(last_flash(&mock), Some(MZ_MACHINEGUN));
        let client = world.client(key).unwrap();
        assert_eq!(client.machinegun_shots, 3);
        assert_eq!(
            client.kick_angles.x, -3.0,
            "two shots climbed before this one"
        );

        hold(&mut world, key, false);
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().machinegun_shots, 0);
        assert_eq!(inventory(&world, key, "Bullets"), 47);
    }

    #[test]
    fn the_machinegun_climb_stops_at_nine_and_not_in_deathmatch() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Machinegun", "Bullets", 50);
        hold(&mut world, key, true);
        for _ in 0..15 {
            world.think_weapon(key);
        }
        assert_eq!(world.client(key).unwrap().machinegun_shots, 9);

        let (mut world, key, _) = ready();
        world.game.deathmatch = true;
        equip(&mut world, key, "Machinegun", "Bullets", 50);
        hold(&mut world, key, true);
        think_n(&mut world, key, 3);
        assert_eq!(world.client(key).unwrap().machinegun_shots, 0);
    }

    #[test]
    fn an_empty_machinegun_clicks_and_switches() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Machinegun", "Bullets", 1);
        hold(&mut world, key, true);
        think_n(&mut world, key, 2);
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.gunframe, 6);
        assert_eq!(client.newweapon, world.find_item_by_name("Blaster"));
    }

    #[test]
    fn the_chaingun_spins_up_from_one_to_three_shots_a_frame() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Chaingun", "Bullets", 200);
        hold(&mut world, key, true);
        think_n(&mut world, key, 4); // frames 6..9: one each
        assert_eq!(inventory(&world, key, "Bullets"), 196);
        think_n(&mut world, key, 5); // frames 10..14: two each
        assert_eq!(inventory(&world, key, "Bullets"), 186);
        assert_eq!(last_flash(&mock), Some(MZ_CHAINGUN2));
        world.think_weapon(key); // frame 15: three
        assert_eq!(inventory(&world, key, "Bullets"), 183);
        assert_eq!(last_flash(&mock), Some(MZ_CHAINGUN3));
        let loop_sound = world.gi.sound_index("weapons/chngnl1a.wav");
        assert_eq!(world.client(key).unwrap().weapon_sound, loop_sound);
        think_n(&mut world, key, 6); // 16..21
        assert_eq!(world.client(key).unwrap().ps.gunframe, 21);
        world.think_weapon(key); // 21 loops back to 15 while held
        assert_eq!(world.client(key).unwrap().ps.gunframe, 15);
    }

    #[test]
    fn letting_go_of_the_chaingun_at_frame_14_winds_it_down() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Chaingun", "Bullets", 200);
        hold(&mut world, key, true);
        think_n(&mut world, key, 9);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 14);
        hold(&mut world, key, false);
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.gunframe, 32);
        assert_eq!(client.weapon_sound, 0);
    }

    #[test]
    fn the_hyperblaster_streams_bolts_and_loops_while_held() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "HyperBlaster", "Cells", 50);
        hold(&mut world, key, true);
        think_n(&mut world, key, 6); // frames 6..11
        assert_eq!(inventory(&world, key, "Cells"), 44);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 6, "12 loops to 6");
        let bolts = spawned(&world, "bolt");
        assert_eq!(bolts.len(), 6);
        assert!(bolts.iter().all(|b| b.game.dmg == 20));
        assert_eq!(
            bolts
                .iter()
                .filter(|b| b.state.effects & EF_HYPERBLASTER != 0)
                .count(),
            2,
            "frames 6 and 9 glow"
        );
        let hum = world.gi.sound_index("weapons/hyprbl1a.wav");
        assert_eq!(world.client(key).unwrap().weapon_sound, hum);

        hold(&mut world, key, false);
        think_n(&mut world, key, 6);
        assert_eq!(
            world.client(key).unwrap().weapon_sound,
            0,
            "wound down at 12"
        );
    }

    #[test]
    fn the_hyperblaster_barrel_offset_matches_c_double_math_on_frame_10() {
        // C stores (10 - 5) * 2 * M_PI / 6 (double) in a float, then takes
        // double sin/cos; the all-f32 chain rounds the rotation differently.
        let offset = hyperblaster_offset(10);
        assert_eq!(offset.x.to_bits(), 3.464_101_8_f32.to_bits());
        assert_eq!(offset.y, 0.0);
        assert_eq!(offset.z.to_bits(), 1.999_999_6_f32.to_bits());
    }

    #[test]
    fn the_rocket_launcher_fires_one_rocket() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Rocket Launcher", "Rockets", 5);
        press(&mut world, key);
        assert_eq!(last_flash(&mock), Some(MZ_ROCKET));
        assert_eq!(inventory(&world, key, "Rockets"), 4);
        let rockets = spawned(&world, "rocket");
        assert_eq!(rockets.len(), 1);
        assert!(
            (100..120).contains(&rockets[0].game.dmg),
            "{}",
            rockets[0].game.dmg
        );
    }

    #[test]
    fn the_grenade_launcher_lobs_a_grenade() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Grenade Launcher", "Grenades", 5);
        press(&mut world, key);
        assert_eq!(last_flash(&mock), Some(MZ_GRENADE));
        assert_eq!(inventory(&world, key, "Grenades"), 4);
        let grenades = spawned(&world, "grenade");
        assert_eq!(grenades.len(), 1);
        assert_eq!(
            (grenades[0].game.dmg, grenades[0].game.dmg_radius),
            (120, 160.0)
        );
    }

    #[test]
    fn the_railgun_fires_one_slug_with_a_big_kick() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Railgun", "Slugs", 3);
        press(&mut world, key);
        assert_eq!(last_flash(&mock), Some(MZ_RAILGUN));
        assert_eq!(inventory(&world, key, "Slugs"), 2);
        assert_eq!(world.client(key).unwrap().kick_angles.x, -3.0);
    }

    #[test]
    fn the_bfg_flashes_on_frame_9_and_fires_on_17_for_50_cells() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "BFG10K", "Cells", 60);
        press(&mut world, key);
        assert_eq!(last_flash(&mock), Some(MZ_BFG));
        assert!(spawned(&world, "bfg blast").is_empty());
        think_n(&mut world, key, 7); // 10..16
        assert!(spawned(&world, "bfg blast").is_empty());
        world.think_weapon(key); // 17
        assert_eq!(spawned(&world, "bfg blast").len(), 1);
        assert_eq!(inventory(&world, key, "Cells"), 10);
        assert_eq!(world.client(key).unwrap().v_dmg_pitch, -40.0);
    }

    #[test]
    fn the_bfg_holds_fire_when_cells_drop_during_the_windup() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "BFG10K", "Cells", 50);
        press(&mut world, key);
        let cells = world.find_item_by_name("Cells").unwrap();
        world.client_mut(key).unwrap().pers.inventory[cells] = 49;
        think_n(&mut world, key, 8);
        assert!(spawned(&world, "bfg blast").is_empty());
        assert_eq!(inventory(&world, key, "Cells"), 49);
    }

    fn silence(world: &mut GameWorld, key: EntityKey) {
        world.client_mut(key).unwrap().silencer_shots = 30;
    }

    fn silencer_shots(world: &GameWorld, key: EntityKey) -> i32 {
        world.client(key).unwrap().silencer_shots
    }

    #[test]
    fn one_trigger_of_each_gun_uses_one_silencer_charge() {
        // C: each player fire routine calls PlayerNoise(PNOISE_WEAPON) once.
        for (weapon, ammo) in [
            ("Blaster", "Cells"),
            ("Shotgun", "Shells"),
            ("Super Shotgun", "Shells"),
            ("Machinegun", "Bullets"),
            ("Rocket Launcher", "Rockets"),
            ("Grenade Launcher", "Grenades"),
            ("Railgun", "Slugs"),
        ] {
            let (mut world, key, _) = ready();
            equip(&mut world, key, weapon, ammo, 10);
            silence(&mut world, key);
            hold(&mut world, key, true); // the machinegun fires while held
            press(&mut world, key);
            assert_eq!(silencer_shots(&world, key), 29, "{weapon}");
        }
    }

    #[test]
    fn a_three_shot_chaingun_frame_uses_one_silencer_charge() {
        let (mut world, key, mock) = ready();
        equip(&mut world, key, "Chaingun", "Bullets", 200);
        hold(&mut world, key, true);
        think_n(&mut world, key, 9); // frames 6..14
        silence(&mut world, key);
        world.think_weapon(key); // frame 15: three
        assert_eq!(last_flash(&mock), Some(MZ_CHAINGUN3 | MZ_SILENCED));
        assert_eq!(silencer_shots(&world, key), 29);
    }

    #[test]
    fn a_hyperblaster_bolt_uses_one_silencer_charge() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "HyperBlaster", "Cells", 50);
        silence(&mut world, key);
        hold(&mut world, key, true);
        press(&mut world, key);
        assert_eq!(bolts(&world), 1);
        assert_eq!(silencer_shots(&world, key), 29);
    }

    #[test]
    fn the_bfg_windup_and_launch_each_make_a_weapon_noise() {
        // C weapon_bfg_fire: PlayerNoise on frame 9 and again on frame 17.
        let (mut world, key, _) = ready();
        equip(&mut world, key, "BFG10K", "Cells", 60);
        silence(&mut world, key);
        press(&mut world, key); // 9
        assert_eq!(silencer_shots(&world, key), 29);
        think_n(&mut world, key, 8); // 10..17
        assert_eq!(spawned(&world, "bfg blast").len(), 1);
        assert_eq!(silencer_shots(&world, key), 28);
    }

    #[test]
    fn the_bfg_windup_alone_is_heard() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "BFG10K", "Cells", 60);
        world.level.framenum = 100;
        press(&mut world, key); // 9
        let noise = world.level.sound_entity.expect("the windup is heard");
        assert_eq!(world.entities.get(noise).unwrap().owner, Some(key));
        assert!(spawned(&world, "bfg blast").is_empty());
    }

    #[test]
    fn a_hand_grenade_throw_makes_no_weapon_noise() {
        // C weapon_grenade_fire never calls PlayerNoise(PNOISE_WEAPON).
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Grenades", "Grenades", 5);
        silence(&mut world, key);
        hold(&mut world, key, true);
        press(&mut world, key);
        hold(&mut world, key, false);
        think_n(&mut world, key, 20);
        assert!(!spawned(&world, "hgrenade").is_empty());
        assert_eq!(silencer_shots(&world, key), 30);
    }

    #[test]
    fn infinite_ammo_dmflag_keeps_the_ammo() {
        let (mut world, key, _) = ready();
        world.game.dmflags = DF_INFINITE_AMMO;
        equip(&mut world, key, "Railgun", "Slugs", 3);
        press(&mut world, key);
        assert_eq!(inventory(&world, key, "Slugs"), 3);
    }

    #[test]
    fn a_hand_grenade_cooks_while_held_and_flies_on_release() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Grenades", "Grenades", 5);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 16);
        hold(&mut world, key, true);
        press(&mut world, key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 1);
        think_n(&mut world, key, 12); // up to 11, where it waits
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.gunframe, 11);
        assert_eq!(client.grenade_time, world.level.time + 3.2);
        assert!(spawned(&world, "hgrenade").is_empty());

        hold(&mut world, key, false);
        think_n(&mut world, key, 2);
        let thrown = spawned(&world, "hgrenade");
        assert_eq!(thrown.len(), 1);
        assert_eq!(thrown[0].game.spawnflags, 1);
        assert_eq!(inventory(&world, key, "Grenades"), 4);
        // Frame 15 waits out the second before the next throw.
        think_n(&mut world, key, 5);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 15);
        world.level.time += 1.0;
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert_eq!(client.weaponstate, WeaponState::Ready);
        assert_eq!(client.grenade_time, 0.0);
    }

    #[test]
    fn a_hand_grenade_held_too_long_goes_off_in_the_hand() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Grenades", "Grenades", 5);
        hold(&mut world, key, true);
        press(&mut world, key);
        think_n(&mut world, key, 12);
        world.level.time += 3.2;
        world.think_weapon(key);
        let client = world.client(key).unwrap();
        assert!(client.grenade_blew_up);
        assert_eq!(inventory(&world, key, "Grenades"), 4);
        // A zero timer explodes at once: nothing left flying.
        assert!(spawned(&world, "hgrenade").is_empty());
    }

    #[test]
    fn switching_away_with_a_cooked_grenade_sets_it_off() {
        // C ChangeWeapon fires it with grenade_time = now: a zero timer.
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Grenades", "Grenades", 5);
        hold(&mut world, key, true);
        press(&mut world, key);
        think_n(&mut world, key, 12);
        world.client_mut(key).unwrap().newweapon = world.find_item_by_name("Blaster");
        world.change_weapon(key);
        assert_eq!(inventory(&world, key, "Grenades"), 4);
        assert!(spawned(&world, "hgrenade").is_empty(), "exploded at once");
        let client = world.client(key).unwrap();
        assert_eq!(client.grenade_time, 0.0);
        assert_eq!(client.pers.weapon, world.find_item_by_name("Blaster"));
    }

    #[test]
    fn hand_grenades_put_weap_grenades_in_the_skin_for_vwep() {
        let (mut world, key, _) = ready();
        world.entities.get_mut(key).unwrap().state.modelindex = 255;
        equip(&mut world, key, "Grenades", "Grenades", 5);
        let skinnum = world.entities.get(key).unwrap().state.skinnum;
        assert_eq!(skinnum >> 8, crate::items::WEAP_GRENADES);
    }

    #[test]
    fn changing_weapon_resets_the_machinegun_climb() {
        let (mut world, key, _) = ready();
        let blaster = world.find_item_by_name("Blaster");
        let client = world.client_mut(key).unwrap();
        client.machinegun_shots = 7;
        client.newweapon = blaster;
        world.change_weapon(key);
        assert_eq!(world.client(key).unwrap().machinegun_shots, 0);
    }

    #[test]
    fn a_ready_hand_grenade_switches_straight_away() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Grenades", "Grenades", 5);
        let blaster = world.find_item_by_name("Blaster");
        world.client_mut(key).unwrap().newweapon = blaster;
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().pers.weapon, blaster);
    }

    #[test]
    fn without_grenades_a_press_clicks_and_switches() {
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Grenades", "Grenades", 1);
        let grenades = world.find_item_by_name("Grenades").unwrap();
        world.client_mut(key).unwrap().pers.inventory[grenades] = 0;
        press(&mut world, key);
        assert_eq!(
            world.client(key).unwrap().newweapon,
            world.find_item_by_name("Blaster")
        );
    }

    #[test]
    fn no_ammo_change_prefers_the_strongest_loaded_weapon() {
        let (mut world, key, _) = ready();
        for (item, count) in [
            ("Shotgun", 1),
            ("Shells", 5),
            ("Machinegun", 1),
            ("Bullets", 5),
        ] {
            let index = world.find_item_by_name(item).unwrap();
            world.client_mut(key).unwrap().pers.inventory[index] = count;
        }
        world.no_ammo_weapon_change(key);
        assert_eq!(
            world.client(key).unwrap().newweapon,
            world.find_item_by_name("Machinegun")
        );
    }

    // ---- C boundaries and offsets ----

    #[test]
    fn a_player_on_1_health_keeps_the_weapon() {
        // C ref: Think_Weapon, health < 1 puts it away.
        let (mut world, key, _) = ready();
        world.entities.get_mut(key).unwrap().game.health = 1;
        let weapon = world.client(key).unwrap().pers.weapon;
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().pers.weapon, weapon);
    }

    #[test]
    fn exactly_enough_ammo_for_a_shot_selects_the_weapon() {
        // C ref: Use_Weapon, inventory < quantity is "Not enough".
        let (mut world, key, _) = armed();
        let ssg = world.find_item_by_name("Super Shotgun").unwrap();
        let shells = world.find_item_by_name("Shells").unwrap();
        let client = world.client_mut(key).unwrap();
        client.pers.inventory[ssg] = 1;
        client.pers.inventory[shells] = 2;
        use_weapon(&mut world, key, ssg);
        assert_eq!(world.client(key).unwrap().newweapon, Some(ssg));
    }

    #[test]
    fn a_ducked_player_lowers_the_gun_with_the_crouch_pain_frames() {
        let (mut world, key, _) = armed();
        world.client_mut(key).unwrap().ps.pmove.pm_flags |= PMF_DUCKED;
        change_weap_animation(&mut world, key);
        assert_eq!(
            world.entities.get(key).unwrap().state.frame,
            FRAME_CRPAIN4 + 1
        );
        assert_eq!(world.client(key).unwrap().anim_end, FRAME_CRPAIN1);
    }

    #[test]
    fn a_long_lowering_starts_the_reverse_animation_four_frames_from_the_end() {
        // C ref: Weapon_Generic, the railgun lowers over 57..61.
        let (mut world, key, _) = ready();
        equip(&mut world, key, "Railgun", "Slugs", 10);
        world.client_mut(key).unwrap().newweapon = world.find_item_by_name("Blaster");
        let reverse = |world: &GameWorld| world.client(key).unwrap().anim_priority == ANIM_REVERSE;
        world.think_weapon(key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 57);
        assert!(!reverse(&world), "not at once");
        world.think_weapon(key);
        assert!(reverse(&world), "on leaving frame 57");
    }

    #[test]
    fn quad_ending_this_frame_makes_no_quad_sound() {
        let (mut world, key, mock) = ready();
        let framenum = world.level.framenum;
        let client = world.client_mut(key).unwrap();
        client.latched_buttons = BUTTON_ATTACK;
        client.quad_framenum = framenum;
        think_n(&mut world, key, 2);
        let damage3 = world.gi.sound_index("items/damage3.wav");
        assert!(!mock.sound_calls.borrow().iter().any(|c| c.2 == damage3));
    }

    #[test]
    fn a_bolt_leaves_from_the_blaster_offset_plus_the_barrel_offset() {
        // C ref: Blaster_Fire, offset (24, 8, viewheight - 8) + g_offset.
        let (mut world, key, _) = armed();
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::ZERO;
        ent.game.viewheight = 22.0;
        let client = world.client_mut(key).unwrap();
        client.v_angle = Vec3f::ZERO;
        client.pers.hand = RIGHT_HANDED;
        blaster_fire(&mut world, key, Vec3f::new(1.0, 2.0, 3.0), 10, false, 0);
        let bolt = spawned(&world, "bolt")[0];
        // Right-handed at yaw 0: right is -y.
        assert_eq!(bolt.state.old_origin, Vec3f::new(25.0, -10.0, 17.0));
        assert_eq!(
            world.client(key).unwrap().kick_origin,
            Vec3f::new(-2.0, 0.0, 0.0)
        );
    }

    /// A right-handed player at (10, 20, 30), eyes 22 up, facing +x, with
    /// `ammo` loaded; the trace log starts empty.
    fn gunner(ammo: &str) -> (GameWorld, EntityKey, Rc<MockGameImport>) {
        let (mut world, key, gi) = armed();
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::new(10.0, 20.0, 30.0);
        ent.game.viewheight = 22.0;
        let ammo = world.find_item_by_name(ammo).unwrap();
        let client = world.client_mut(key).unwrap();
        client.v_angle = Vec3f::ZERO;
        client.pers.hand = RIGHT_HANDED;
        client.ammo_index = ammo;
        client.pers.inventory[ammo] = 50;
        client.buttons = BUTTON_ATTACK;
        gi.trace_ends.borrow_mut().clear();
        (world, key, gi)
    }

    /// The next `n` crandom() values a world seeded with `seed` draws.
    fn crandoms(seed: u32, n: usize) -> Vec<f32> {
        let mut world = test_world();
        world.level.rand_state = seed;
        (0..n).map(|_| world.crandom()).collect()
    }

    /// Far ends of the bullet traces (the first trace of each bullet goes
    /// to its start), relative to `start`.
    fn bullet_ends(gi: &MockGameImport, start: Vec3f) -> Vec<Vec3f> {
        let ends = gi.trace_ends.borrow();
        ends.iter()
            .map(|&end| end - start)
            .filter(|end| end.length() > 1000.0)
            .collect()
    }

    #[test]
    fn shotguns_fire_from_8_right_and_8_below_the_eye_and_kick_back_2() {
        // C ref: weapon_shotgun_fire / weapon_supershotgun_fire, offset
        // (0, 8, viewheight - 8), kick_origin = forward * -2.
        for fire in [
            weapon_shotgun_fire as WeaponFireFn,
            weapon_supershotgun_fire,
        ] {
            let (mut world, key, gi) = gunner("Shells");
            fire(&mut world, key);
            assert_eq!(gi.trace_ends.borrow()[0], Vec3f::new(10.0, 12.0, 44.0));
            let client = world.client(key).unwrap();
            assert_eq!(client.kick_origin, Vec3f::new(-2.0, 0.0, 0.0));
            assert_eq!(client.kick_angles.x, -2.0);
        }
    }

    #[test]
    fn the_super_shotgun_fires_ten_pellets_5_degrees_right_then_ten_left() {
        // C ref: weapon_supershotgun_fire, yaw - 5 then yaw + 5, count / 2.
        let (mut world, key, gi) = gunner("Shells");
        world.level.rand_state = 77;
        weapon_supershotgun_fire(&mut world, key);
        let ends = bullet_ends(&gi, Vec3f::new(10.0, 12.0, 44.0));
        assert_eq!(ends.len(), 20);
        let mean_y = |ends: &[Vec3f]| ends.iter().map(|e| e.y).sum::<f32>() / 10.0;
        assert!(mean_y(&ends[..10]) < 0.0, "{}", mean_y(&ends[..10]));
        assert!(mean_y(&ends[10..]) > 0.0, "{}", mean_y(&ends[10..]));
    }

    #[test]
    fn a_machinegun_shot_kicks_by_crandom_in_c_order_and_aims_with_the_kick() {
        // C ref: Machinegun_Fire, kick_origin[1], kick_angles[1],
        // kick_origin[2], kick_angles[2], then kick_origin[0].
        let (mut world, key, gi) = gunner("Bullets");
        let client = world.client_mut(key).unwrap();
        client.v_angle = Vec3f::new(10.0, 90.0, 0.0);
        client.machinegun_shots = 2;
        client.ps.gunframe = 4;
        world.level.rand_state = 4321;
        let j = crandoms(4321, 5);
        machinegun_fire(&mut world, key);
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.gunframe, 5);
        let kick_origin = Vec3f::new(j[4] * 0.35, j[0] * 0.35, j[2] * 0.35);
        assert_eq!(client.kick_origin, kick_origin);
        let kick_angles = Vec3f::new(-3.0, j[1] * 0.7, j[3] * 0.7);
        assert_eq!(client.kick_angles, kick_angles);
        let (forward, right, _) =
            crate::physics::angle_vectors(Vec3f::new(10.0, 90.0, 0.0) + kick_angles);
        let start = world.player_project_source(key, [0.0, 8.0, 14.0], forward, right);
        assert_eq!(gi.trace_ends.borrow()[0], start);
    }

    #[test]
    fn the_machinegun_alternates_frames_4_and_5_and_steps_on_release() {
        let (mut world, key, _) = gunner("Bullets");
        world.client_mut(key).unwrap().ps.gunframe = 5;
        machinegun_fire(&mut world, key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 4);
        world.client_mut(key).unwrap().buttons = 0;
        machinegun_fire(&mut world, key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 5);
    }

    #[test]
    fn the_machinegun_attack_animation_starts_a_frame_early_a_quarter_of_the_time() {
        // C ref: Machinegun_Fire, (int)(random() + 0.25) frames back.
        let mut frames = Vec::new();
        for seed in (0..16).map(|n| n * 997) {
            let (mut world, key, _) = gunner("Bullets");
            world.level.rand_state = seed;
            machinegun_fire(&mut world, key);
            frames.push(world.entities.get(key).unwrap().state.frame);
        }
        assert!(frames.contains(&FRAME_ATTACK1), "{frames:?}");
        assert!(frames.contains(&(FRAME_ATTACK1 - 1)), "{frames:?}");
    }

    #[test]
    fn a_chaingun_burst_kicks_and_spreads_its_muzzle_by_crandom_in_c_order() {
        // C ref: Chaingun_Fire, kick_origin[i]/kick_angles[i] for i 0..3,
        // then per shot r = 7 + crandom() * 4, u = crandom() * 4.
        let (mut world, key, gi) = gunner("Bullets");
        world.client_mut(key).unwrap().ps.gunframe = 5;
        world.level.rand_state = 99;
        let j = crandoms(99, 8);
        chaingun_fire(&mut world, key);
        let client = world.client(key).unwrap();
        assert_eq!(
            client.kick_origin,
            Vec3f::new(j[0] * 0.35, j[2] * 0.35, j[4] * 0.35)
        );
        assert_eq!(
            client.kick_angles,
            Vec3f::new(j[1] * 0.7, j[3] * 0.7, j[5] * 0.7)
        );
        let (start, _, _) = world.shot_source(key, [0.0, 7.0 + j[6] * 4.0, j[7] * 4.0 + 14.0]);
        assert_eq!(gi.trace_ends.borrow()[0], start);
    }

    #[test]
    fn the_chaingun_spins_up_with_a_sound_on_frame_5_only() {
        let spun = |gunframe: i32| {
            let (mut world, key, gi) = gunner("Bullets");
            world.client_mut(key).unwrap().ps.gunframe = gunframe;
            chaingun_fire(&mut world, key);
            let spin_up = world.gi.sound_index("weapons/chngnu1a.wav");
            let calls = gi.sound_calls.borrow();
            calls.iter().any(|c| c.2 == spin_up)
        };
        assert!(spun(5));
        assert!(!spun(6));
    }

    #[test]
    fn an_empty_chaingun_does_not_loop_back_from_frame_21() {
        // C ref: Chaingun_Fire, frame 21 loops to 15 only with ammo.
        let (mut world, key, _) = gunner("Bullets");
        let client = world.client_mut(key).unwrap();
        client.ps.gunframe = 21;
        let ammo = client.ammo_index;
        client.pers.inventory[ammo] = 0;
        chaingun_fire(&mut world, key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 22);
    }

    #[test]
    fn chaingun_frames_10_to_14_fire_two_shots_only_while_held() {
        for (held, shots) in [(true, 2), (false, 1)] {
            let (mut world, key, _) = gunner("Bullets");
            let client = world.client_mut(key).unwrap();
            client.ps.gunframe = 10;
            client.buttons = if held { BUTTON_ATTACK } else { 0 };
            chaingun_fire(&mut world, key);
            assert_eq!(inventory(&world, key, "Bullets"), 50 - shots, "{held}");
        }
    }

    #[test]
    fn the_chaingun_attack_animation_backs_up_on_odd_frames() {
        // C ref: Chaingun_Fire, ent->s.frame = FRAME_attack1 - (gunframe & 1).
        for (gunframe, frame) in [(6, FRAME_ATTACK1 - 1), (7, FRAME_ATTACK1)] {
            let (mut world, key, _) = gunner("Bullets");
            world.client_mut(key).unwrap().ps.gunframe = gunframe;
            chaingun_fire(&mut world, key);
            assert_eq!(world.entities.get(key).unwrap().state.frame, frame);
        }
    }

    #[test]
    fn the_last_hyperblaster_cell_ends_the_loop() {
        // C ref: Weapon_HyperBlaster_Fire, frame 12 loops to 6 only with ammo.
        let (mut world, key, _) = gunner("Cells");
        let client = world.client_mut(key).unwrap();
        client.ps.gunframe = 11;
        let cells = client.ammo_index;
        client.pers.inventory[cells] = 1;
        weapon_hyperblaster_fire(&mut world, key);
        assert_eq!(world.client(key).unwrap().ps.gunframe, 12);
    }

    fn quad(world: &mut GameWorld, key: EntityKey) {
        let framenum = world.level.framenum;
        world.client_mut(key).unwrap().quad_framenum = framenum + 100;
    }

    fn only(world: &GameWorld, classname: &str) -> crate::entity::Entity {
        let found = spawned(world, classname);
        assert_eq!(found.len(), 1, "{classname}");
        found[0].clone()
    }

    /// 8 forward, 8 right and 14 up from the gunner's origin.
    const LAUNCHER_START: Vec3f = Vec3f::new(18.0, 12.0, 44.0);

    #[test]
    fn a_rocket_does_100_to_119_from_the_launcher_muzzle_with_a_kick() {
        // C ref: Weapon_RocketLauncher_Fire, 100 + (int)(random() * 20.0).
        let (mut world, key, _) = gunner("Rockets");
        world.level.rand_state = 5;
        let r = {
            let mut scratch = test_world();
            scratch.level.rand_state = 5;
            scratch.random()
        };
        weapon_rocketlauncher_fire(&mut world, key);
        let rocket = only(&world, "rocket");
        assert_eq!(rocket.game.dmg, 100 + (r * 20.0) as i32);
        assert!(rocket.game.dmg > 100, "seed 5 draws over 0.05");
        assert_eq!(rocket.state.origin, LAUNCHER_START);
        let client = world.client(key).unwrap();
        assert_eq!(
            (client.kick_origin, client.kick_angles.x),
            (Vec3f::new(-2.0, 0.0, 0.0), -1.0)
        );
    }

    #[test]
    fn quad_damage_quadruples_a_rockets_hit_and_splash() {
        let (mut world, key, _) = gunner("Rockets");
        quad(&mut world, key);
        weapon_rocketlauncher_fire(&mut world, key);
        let rocket = only(&world, "rocket");
        assert_eq!(rocket.game.dmg % 4, 0);
        assert!((400..480).contains(&rocket.game.dmg), "{}", rocket.game.dmg);
        assert_eq!(rocket.game.radius_dmg, 480);
    }

    #[test]
    fn a_launcher_grenade_leaves_the_launcher_muzzle_with_a_kick() {
        let (mut world, key, _) = gunner("Grenades");
        weapon_grenadelauncher_fire(&mut world, key);
        assert_eq!(only(&world, "grenade").state.origin, LAUNCHER_START);
        assert_eq!(world.client(key).unwrap().kick_angles.x, -1.0);
    }

    #[test]
    fn a_rail_leaves_7_right_and_quad_quadruples_its_damage_and_knockback() {
        // C ref: weapon_railgun_fire, offset (0, 7, viewheight - 8),
        // single player 150 damage and 250 kick, x4 with quad.
        let (mut world, key, gi) = gunner("Slugs");
        quad(&mut world, key);
        let target = world.spawn().unwrap();
        let t = world.entities.get_mut(target).unwrap();
        t.game.takedamage = TakeDamage::Yes;
        t.game.health = 1000;
        t.game.mass = 200;
        t.game.movetype = MoveType::Step;
        t.solid = q2_shared::types::Solid::Bbox;
        *gi.next_trace.borrow_mut() = Some(q2_shared::types::Trace {
            fraction: 0.5,
            ent_index: world.number_of(target),
            ..q2_shared::types::Trace::default()
        });
        weapon_railgun_fire(&mut world, key);
        assert!(
            gi.vectors.borrow().contains(&Vec3f::new(10.0, 13.0, 44.0)),
            "rail trail start"
        );
        let t = world.entities.get(target).unwrap();
        assert_eq!(t.game.health, 400);
        // Knockback 1000 on mass 200: 500 * 1000 / 200 along +x.
        assert!((t.velocity.x - 2500.0).abs() < 1e-2, "{}", t.velocity.x);
    }

    #[test]
    fn the_bfg_windup_noise_comes_from_its_muzzle() {
        let (mut world, key, _) = gunner("Cells");
        world.level.framenum = 10;
        world.level.sound_entity_framenum = -10;
        world.client_mut(key).unwrap().ps.gunframe = 9;
        weapon_bfg_fire(&mut world, key);
        let noise = world.entities.get(key).unwrap().game.mynoise.unwrap();
        assert_eq!(
            world.entities.get(noise).unwrap().state.origin,
            LAUNCHER_START
        );
    }

    #[test]
    fn the_bfg_fires_on_exactly_50_cells_with_quad_and_a_rolled_view_kick() {
        // C ref: weapon_bfg_fire, cells < 50 holds fire; v_dmg_roll =
        // crandom() * 8; v_dmg_time = level.time + DAMAGE_TIME.
        let (mut world, key, _) = gunner("Cells");
        quad(&mut world, key);
        world.level.time = 5.0;
        world.level.rand_state = 31;
        let j = crandoms(31, 1);
        world.client_mut(key).unwrap().ps.gunframe = 17;
        weapon_bfg_fire(&mut world, key);
        let bfg = only(&world, "bfg blast");
        assert_eq!(bfg.state.origin, LAUNCHER_START);
        assert_eq!(bfg.game.radius_dmg, 2000);
        let client = world.client(key).unwrap();
        assert_eq!(client.kick_origin, Vec3f::new(-2.0, 0.0, 0.0));
        assert_eq!(client.v_dmg_roll, j[0] * 8.0);
        assert_eq!(client.v_dmg_time, 5.0 + DAMAGE_TIME);
        assert_eq!(inventory(&world, key, "Cells"), 0);
    }

    #[test]
    fn a_thrown_grenade_flies_faster_the_longer_it_was_held() {
        // C ref: weapon_grenade_fire, speed = 400 + (3 - timer) * 400 / 3,
        // radius = damage + 40, damage x4 with quad.
        let (mut world, key, _) = gunner("Grenades");
        quad(&mut world, key);
        world.level.time = 10.0;
        world.client_mut(key).unwrap().grenade_time = 12.0;
        weapon_grenade_fire(&mut world, key, false);
        let grenade = only(&world, "hgrenade");
        assert_eq!(grenade.state.origin, LAUNCHER_START);
        assert_eq!(grenade.velocity.x, 533.0);
        assert_eq!((grenade.game.dmg, grenade.game.dmg_radius), (500, 165.0));
    }

    #[test]
    fn a_grenade_throw_waves_standing_and_attacks_crouched() {
        // C ref: weapon_grenade_fire's throw animation.
        let (mut world, key, _) = gunner("Grenades");
        world.client_mut(key).unwrap().grenade_time = world.level.time + 2.0;
        weapon_grenade_fire(&mut world, key, false);
        assert_eq!(world.entities.get(key).unwrap().state.frame, FRAME_WAVE08);
        assert_eq!(world.client(key).unwrap().anim_priority, ANIM_REVERSE);

        let (mut world, key, _) = gunner("Grenades");
        world.client_mut(key).unwrap().ps.pmove.pm_flags |= PMF_DUCKED;
        weapon_grenade_fire(&mut world, key, false);
        assert_eq!(
            world.entities.get(key).unwrap().state.frame,
            FRAME_CRATTAK1 - 1
        );

        // Not in a vwep model: no animation.
        let (mut world, key, _) = gunner("Grenades");
        world.entities.get_mut(key).unwrap().state.modelindex = 0;
        world.entities.get_mut(key).unwrap().state.frame = 3;
        weapon_grenade_fire(&mut world, key, false);
        assert_eq!(world.entities.get(key).unwrap().state.frame, 3);
    }

    /// Hand grenades up and ready on `gunframe`, the button up.
    fn grenadier(gunframe: i32) -> (GameWorld, EntityKey, Rc<MockGameImport>) {
        let (mut world, key, gi) = gunner("Grenades");
        let grenades = world.find_item_by_name("Grenades").unwrap();
        let client = world.client_mut(key).unwrap();
        client.pers.weapon = Some(grenades);
        client.weaponstate = WeaponState::Ready;
        client.ps.gunframe = gunframe;
        client.buttons = 0;
        (world, key, gi)
    }

    #[test]
    fn ready_hand_grenades_idle_through_48_and_back_to_16() {
        // C ref: Weapon_Grenade, gunframe++ then > 48 wraps to 16.
        for (from, to) in [(30, 31), (47, 48)] {
            let (mut world, key, _) = grenadier(from);
            weapon_grenade(&mut world, key);
            assert_eq!(world.client(key).unwrap().ps.gunframe, to, "{from}");
            assert_eq!(world.client(key).unwrap().weaponstate, WeaponState::Ready);
        }
    }

    #[test]
    fn ready_hand_grenades_pause_on_their_pause_frames_unless_rand_and_15_is_0() {
        // C ref: Weapon_Grenade, pause_frames && (randk() & 15) holds.
        let mut moved = (false, false);
        for seed in 0..64 {
            let mut scratch = test_world();
            scratch.level.rand_state = seed;
            let steps = scratch.random_u32() & 15 == 0;
            let (mut world, key, _) = grenadier(48);
            world.level.rand_state = seed;
            weapon_grenade(&mut world, key);
            let expected = if steps { 16 } else { 48 };
            assert_eq!(world.client(key).unwrap().ps.gunframe, expected, "{seed}");
            if steps {
                moved.0 = true;
            } else {
                moved.1 = true;
            }
        }
        assert_eq!(moved, (true, true));
    }

    #[test]
    fn a_grenade_press_pulls_the_pin_and_clears_the_latched_attack() {
        let (mut world, key, gi) = grenadier(20);
        world.client_mut(key).unwrap().latched_buttons = BUTTON_ATTACK;
        weapon_grenade(&mut world, key);
        let client = world.client(key).unwrap();
        assert_eq!(client.latched_buttons & BUTTON_ATTACK, 0);
        assert_eq!(
            (client.ps.gunframe, client.weaponstate),
            (1, WeaponState::Firing)
        );
        // The pin sound comes on frame 5 only.
        let pin = world.gi.sound_index("weapons/hgrena1b.wav");
        let pins = |gi: &MockGameImport| {
            gi.sound_calls
                .borrow()
                .iter()
                .filter(|c| c.2 == pin)
                .count()
        };
        world.client_mut(key).unwrap().ps.gunframe = 4;
        weapon_grenade(&mut world, key);
        assert_eq!(pins(&gi), 0);
        weapon_grenade(&mut world, key);
        assert_eq!(pins(&gi), 1);
    }

    #[test]
    fn after_blowing_up_in_the_hand_the_throw_ends_once_the_time_is_up() {
        // C ref: Weapon_Grenade, grenade_blew_up and level.time >= grenade_time.
        let (mut world, key, _) = grenadier(11);
        world.level.time = 10.0;
        let client = world.client_mut(key).unwrap();
        client.weaponstate = WeaponState::Firing;
        client.grenade_blew_up = true;
        client.grenade_time = 10.0;
        weapon_grenade(&mut world, key);
        let client = world.client(key).unwrap();
        assert!(!client.grenade_blew_up);
        assert_eq!(
            (client.ps.gunframe, client.weaponstate),
            (16, WeaponState::Ready)
        );
    }

    #[test]
    fn a_quad_chaingun_bullet_does_four_times_8() {
        // C ref: Chaingun_Fire, damage *= 4 with quad.
        let (mut world, key, gi) = gunner("Bullets");
        quad(&mut world, key);
        world.client_mut(key).unwrap().ps.gunframe = 5;
        let target = world.spawn().unwrap();
        let t = world.entities.get_mut(target).unwrap();
        t.game.takedamage = TakeDamage::Yes;
        t.game.health = 1000;
        t.solid = q2_shared::types::Solid::Bbox;
        // The muzzle check runs clear; the bullet hits the target.
        *gi.next_trace.borrow_mut() = Some(q2_shared::types::Trace::default());
        gi.queued_traces
            .borrow_mut()
            .push_back(q2_shared::types::Trace {
                fraction: 0.5,
                ent_index: world.number_of(target),
                ..q2_shared::types::Trace::default()
            });
        chaingun_fire(&mut world, key);
        assert_eq!(world.entities.get(target).unwrap().game.health, 968);
    }
}
