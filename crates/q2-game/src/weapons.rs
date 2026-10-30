//! Weapon fire system — hitscan traces, projectile spawning, impact handlers.
//!
//! Port of the fire functions of `g_weapon.c`, with C's damage, speeds,
//! models and temp entities. The fire sounds are the client's (muzzle
//! flashes).
//! The fire functions also make p_weapon.c's PNOISE_WEAPON `PlayerNoise`
//! at the muzzle when a client fires (a no-op for monsters), so the
//! player's weapon code does not repeat it.
//!
//! # Weapon types
//! - **Hitscan**: fire_bullet, fire_shotgun, fire_rail, fire_hit — instant trace
//! - **Projectile**: fire_blaster, fire_rocket, fire_grenade, fire_bfg — spawns entity
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_weapon.c`

use q2_shared::constants::{
    EF_ANIM_ALLFAST, EF_BFG, EF_GRENADE, EF_ROCKET, SPLASH_BLUE_WATER, SPLASH_BROWN_WATER,
    SPLASH_LAVA, SPLASH_SLIME, SPLASH_UNKNOWN, SURF_FLOWING, SURF_SKY, SURF_TRANS33, SURF_TRANS66,
    SURF_WARP, TE_BFG_BIGEXPLOSION, TE_BFG_EXPLOSION, TE_BFG_LASER, TE_BLASTER, TE_BUBBLETRAIL,
    TE_GRENADE_EXPLOSION, TE_GRENADE_EXPLOSION_WATER, TE_GUNSHOT, TE_LASER_SPARKS, TE_RAILTRAIL,
    TE_ROCKET_EXPLOSION, TE_ROCKET_EXPLOSION_WATER, TE_SHOTGUN,
};
use q2_shared::protocol::SvcOp;
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::physics::angle_vectors;
use crate::player::weapon::PlayerNoise;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// Projectile touch callbacks
// ---------------------------------------------------------------------------

/// A blaster bolt hurts what it hits, or flashes off a wall; into the sky
/// it is just gone. C ref: `blaster_touch` (g_weapon.c).
pub fn blaster_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    plane: Option<&Plane>,
    surface: Option<&Surface>,
) {
    let Some(bolt) = world.entities.get(self_key) else {
        return;
    };
    let owner = bolt.owner;
    if owner == Some(other_key) {
        return;
    }
    if surface.is_some_and(|s| s.flags & SURF_SKY != 0) {
        world.free_entity(self_key);
        return;
    }
    let (origin, velocity, dmg, hyper) = (
        bolt.state.origin,
        bolt.velocity,
        bolt.game.dmg,
        bolt.game.spawnflags & 1 != 0,
    );
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }
    let normal = plane.map_or(Vec3f::ZERO, |p| p.normal);
    let takes_damage = world
        .entities
        .get(other_key)
        .is_some_and(|e| e.game.takedamage != TakeDamage::No);
    if takes_damage {
        let mod_type = if hyper {
            MeansOfDeath::Hyperblaster
        } else {
            MeansOfDeath::Blaster
        };
        world.t_damage(
            other_key,
            self_key,
            owner.unwrap_or(self_key),
            velocity.normalize_or_zero(),
            origin,
            normal,
            dmg,
            1,
            DamageFlags::ENERGY,
            mod_type,
        );
    } else {
        temp_entity(
            world,
            TE_BLASTER,
            &[origin],
            Some(normal),
            origin,
            Multicast::PVS,
        );
    }
    world.free_entity(self_key);
}

/// A rocket hurts what it hits, knocks debris off a solid wall in single
/// player, splashes everything near and explodes. C ref: `rocket_touch`
/// (g_weapon.c).
pub fn rocket_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    plane: Option<&Plane>,
    surface: Option<&Surface>,
) {
    let Some(rocket) = world.entities.get(self_key) else {
        return;
    };
    let owner = rocket.owner;
    if owner == Some(other_key) {
        return;
    }
    if surface.is_some_and(|s| s.flags & SURF_SKY != 0) {
        world.free_entity(self_key);
        return;
    }
    let (dmg, radius_dmg, dmg_radius, origin, velocity, waterlevel) = (
        rocket.game.dmg,
        rocket.game.radius_dmg,
        rocket.game.dmg_radius,
        rocket.state.origin,
        rocket.velocity,
        rocket.game.waterlevel,
    );
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }
    // Where to show the explosion: just back along the flight.
    let blast = origin - velocity * 0.02;
    let takes_damage = world
        .entities
        .get(other_key)
        .is_some_and(|e| e.game.takedamage != TakeDamage::No);
    if takes_damage {
        world.t_damage(
            other_key,
            self_key,
            owner.unwrap_or(self_key),
            velocity.normalize_or_zero(),
            origin,
            plane.map_or(Vec3f::ZERO, |p| p.normal),
            dmg,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Rocket,
        );
    } else if !world.game.deathmatch && !world.game.coop {
        // Don't throw any debris in net games.
        let soft = SURF_WARP | SURF_TRANS33 | SURF_TRANS66 | SURF_FLOWING;
        if surface.is_some_and(|s| s.flags & soft == 0) {
            for _ in 0..world.random_u32() % 5 {
                crate::misc::throw_debris(
                    world,
                    self_key,
                    "models/objects/debris2/tris.md2",
                    2.0,
                    origin,
                );
            }
        }
    }
    world.t_radius_damage(
        self_key,
        owner.unwrap_or(self_key),
        radius_dmg as f32,
        Some(other_key),
        dmg_radius,
        MeansOfDeath::RocketSplash,
    );
    let te = if waterlevel != 0 {
        TE_ROCKET_EXPLOSION_WATER
    } else {
        TE_ROCKET_EXPLOSION
    };
    temp_entity(world, te, &[blast], None, origin, Multicast::PHS);
    world.free_entity(self_key);
}

/// Grenade explosion — called when timer expires or on direct entity hit.
/// The entity a grenade touched (`enemy`) takes one radius-style hit and
/// is left out of the splash. C ref: `Grenade_Explode` (g_weapon.c:510-596).
pub fn grenade_explode(world: &mut GameWorld, self_key: EntityKey) {
    let (dmg, dmg_radius, owner, origin, enemy, spawnflags) = {
        let Some(ent) = world.entities.get(self_key) else {
            return;
        };
        (
            ent.game.dmg,
            ent.game.dmg_radius,
            ent.owner,
            ent.state.origin,
            ent.game.enemy,
            ent.game.spawnflags,
        )
    };
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }
    let attacker = owner.unwrap_or(self_key);

    let enemy_box = enemy
        .and_then(|k| world.entities.get(k))
        .map(|e| (e.state.origin, e.mins, e.maxs));
    if let (Some(enemy), Some((enemy_origin, mins, maxs))) = (enemy, enemy_box) {
        let center = enemy_origin + (mins + maxs) * 0.5;
        let points = dmg as f32 - 0.5 * (origin - center).length();
        world.t_damage(
            enemy,
            self_key,
            attacker,
            enemy_origin - origin,
            origin,
            Vec3f::ZERO,
            points as i32,
            points as i32,
            DamageFlags::RADIUS,
            if spawnflags & 1 != 0 {
                MeansOfDeath::Handgrenade
            } else {
                MeansOfDeath::Grenade
            },
        );
    }

    let splash = if spawnflags & 2 != 0 {
        MeansOfDeath::HeldGrenade
    } else if spawnflags & 1 != 0 {
        MeansOfDeath::HandgrenadeSplash
    } else {
        MeansOfDeath::GrenadeSplash
    };
    world.t_radius_damage(self_key, attacker, dmg as f32, enemy, dmg_radius, splash);

    // A grenade blast on the ground, a rocket blast in the air.
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let blast = ent.state.origin - ent.velocity * 0.02;
    let te = match (ent.game.waterlevel != 0, ent.game.ground_entity.is_some()) {
        (true, true) => TE_GRENADE_EXPLOSION_WATER,
        (true, false) => TE_ROCKET_EXPLOSION_WATER,
        (false, true) => TE_GRENADE_EXPLOSION,
        (false, false) => TE_ROCKET_EXPLOSION,
    };
    temp_entity(world, te, &[blast], None, origin, Multicast::PHS);
    world.free_entity(self_key);
}

/// Grenade think callback — wraps `grenade_explode` for timer detonation.
fn grenade_think(world: &mut GameWorld, self_key: EntityKey) {
    grenade_explode(world, self_key);
}

/// Grenade touch handler — bounces off world, explodes on entities.
/// C ref: `Grenade_Touch` (g_weapon.c:598-644).
pub fn grenade_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    surface: Option<&Surface>,
) {
    let owner = world.entities.get(self_key).and_then(|e| e.owner);
    if owner == Some(other_key) {
        return;
    }
    if surface.is_some_and(|s| s.flags & SURF_SKY != 0) {
        world.free_entity(self_key);
        return;
    }

    let takes_damage = world
        .entities
        .get(other_key)
        .map(|e| e.game.takedamage != TakeDamage::No)
        .unwrap_or(false);
    if !takes_damage {
        // Bounce (MOVETYPE_BOUNCE physics) with a clatter.
        let hand = world
            .entities
            .get(self_key)
            .is_some_and(|e| e.game.spawnflags & 1 != 0);
        let sound = if !hand {
            "weapons/grenlb1b.wav"
        } else if world.random() > 0.5 {
            "weapons/hgrenb1a.wav"
        } else {
            "weapons/hgrenb2a.wav"
        };
        let number = world.number_of(self_key);
        let index = world.gi.sound_index(sound);
        world
            .gi
            .sound(number, CHAN_VOICE, index, 1.0, ATTN_NORM, 0.0);
        return;
    }

    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.enemy = Some(other_key);
    }
    grenade_explode(world, self_key);
}

/// The BFG ball hits: a core blast, then it stops and becomes the
/// explosion sprite that damages everything in view over five frames.
/// C ref: `bfg_touch` (g_weapon.c).
pub fn bfg_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    plane: Option<&Plane>,
    surface: Option<&Surface>,
) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let owner = ent.owner;
    if owner == Some(other_key) {
        return;
    }
    if surface.is_some_and(|s| s.flags & SURF_SKY != 0) {
        world.free_entity(self_key);
        return;
    }
    let (velocity, origin) = (ent.velocity, ent.state.origin);
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }
    let attacker = owner.unwrap_or(self_key);
    // The core explosion: prevents firing it into the wall or floor.
    let takes_damage = world
        .entities
        .get(other_key)
        .is_some_and(|e| e.game.takedamage != TakeDamage::No);
    if takes_damage {
        world.t_damage(
            other_key,
            self_key,
            attacker,
            velocity.normalize_or_zero(),
            origin,
            plane.map_or(Vec3f::ZERO, |p| p.normal),
            200,
            0,
            DamageFlags::empty(),
            MeansOfDeath::BfgBlast,
        );
    }
    world.t_radius_damage(
        self_key,
        attacker,
        200.0,
        Some(other_key),
        100.0,
        MeansOfDeath::BfgBlast,
    );
    let number = world.number_of(self_key);
    let boom = world.gi.sound_index("weapons/bfg__x1b.wav");
    world
        .gi
        .sound(number, CHAN_VOICE, boom, 1.0, ATTN_NORM, 0.0);
    let sprite = world.gi.model_index("sprites/s_bfg3.sp2");
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.solid = Solid::Not;
    ent.touch = None;
    ent.state.origin -= ent.velocity * FRAMETIME;
    ent.velocity = Vec3f::ZERO;
    ent.state.modelindex = sprite;
    ent.state.frame = 0;
    ent.state.sound = 0;
    ent.state.effects &= !EF_ANIM_ALLFAST;
    ent.think = Some(bfg_explode);
    ent.game.nextthink = time + FRAMETIME;
    ent.game.enemy = Some(other_key);
    let at = ent.state.origin;
    temp_entity(world, TE_BFG_BIGEXPLOSION, &[at], None, at, Multicast::PVS);
}

/// The BFG explosion's frames: on the first, everything in `dmg_radius`
/// that it and its owner can see takes energy damage falling off with
/// distance; the sprite is gone after five. C ref: `bfg_explode`.
pub fn bfg_explode(world: &mut GameWorld, self_key: EntityKey) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let (frame, origin, radius, radius_dmg, owner, velocity) = (
        ent.state.frame,
        ent.state.origin,
        ent.game.dmg_radius,
        ent.game.radius_dmg,
        ent.owner,
        ent.velocity,
    );
    if frame == 0 {
        let mut next = None;
        while let Some(target) = world.find_radius(next, origin, radius) {
            next = Some(target);
            let Some(t) = world.entities.get(target) else {
                continue;
            };
            if t.game.takedamage == TakeDamage::No || Some(target) == owner {
                continue;
            }
            let center = t.state.origin + (t.mins + t.maxs) * 0.5;
            let target_origin = t.state.origin;
            if !world.can_damage(target, self_key)
                || !owner.is_some_and(|o| world.can_damage(target, o))
            {
                continue;
            }
            let dist = (origin - center).length();
            let points = radius_dmg as f32 * (1.0 - (dist / radius).sqrt());
            temp_entity(
                world,
                TE_BFG_EXPLOSION,
                &[target_origin],
                None,
                target_origin,
                Multicast::PHS,
            );
            world.t_damage(
                target,
                self_key,
                owner.unwrap_or(self_key),
                velocity,
                target_origin,
                Vec3f::ZERO,
                points as i32,
                0,
                DamageFlags::ENERGY,
                MeansOfDeath::BfgEffect,
            );
        }
    }
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.game.nextthink = time + FRAMETIME;
    ent.state.frame += 1;
    if ent.state.frame == 5 {
        ent.think = Some(projectile_timeout);
    }
}

/// While it flies, the BFG ball lasers every monster, player or exploding
/// box within 256 units, through bodies until something solid. C ref:
/// `bfg_think` (g_weapon.c).
pub fn bfg_think(world: &mut GameWorld, self_key: EntityKey) {
    let dmg = if world.game.deathmatch { 5 } else { 10 };
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let (origin, owner, skinnum) = (ent.state.origin, ent.owner, ent.state.skinnum);
    let mut next = None;
    while let Some(target) = world.find_radius(next, origin, 256.0) {
        next = Some(target);
        if target == self_key || Some(target) == owner {
            continue;
        }
        let Some(t) = world.entities.get(target) else {
            continue;
        };
        let victim = t.svflags & SvFlags::MONSTER.bits() != 0
            || t.client.is_some()
            || t.game.classname == "misc_explobox";
        if t.game.takedamage == TakeDamage::No || !victim {
            continue;
        }
        let point = t.absmin + t.size * 0.5;
        let dir = (point - origin).normalize_or_zero();
        let end = origin + dir * 2048.0;
        let mut start = origin;
        let mut ignore = world.number_of(self_key);
        use q2_shared::constants::{CONTENTS_DEADMONSTER, CONTENTS_MONSTER, CONTENTS_SOLID};
        let mask = CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_DEADMONSTER;
        let mut tr;
        loop {
            tr = world
                .gi
                .trace(start, Vec3f::ZERO, Vec3f::ZERO, end, ignore, mask);
            let Some(hit) = tr.ent_index.and_then(|n| world.entities.key_for_number(n)) else {
                break;
            };
            let Some(h) = world.entities.get(hit) else {
                break;
            };
            let (hurtable, body) = (
                h.game.takedamage != TakeDamage::No
                    && !h.game.flags.contains(EntityFlags::IMMUNE_LASER)
                    && Some(hit) != owner,
                h.svflags & SvFlags::MONSTER.bits() != 0 || h.client.is_some(),
            );
            if hurtable {
                world.t_damage(
                    hit,
                    self_key,
                    owner.unwrap_or(self_key),
                    dir,
                    tr.endpos,
                    Vec3f::ZERO,
                    dmg,
                    1,
                    DamageFlags::ENERGY,
                    MeansOfDeath::BfgLaser,
                );
            }
            // Something that's not a monster or player ends the laser.
            if !body {
                world.gi.write_byte(SvcOp::TempEntity as i32);
                world.gi.write_byte(TE_LASER_SPARKS);
                world.gi.write_byte(4);
                world.gi.write_position(tr.endpos);
                world.gi.write_dir(tr.plane.normal);
                world.gi.write_byte(skinnum);
                world.gi.multicast(tr.endpos, Multicast::PVS);
                break;
            }
            ignore = tr.ent_index;
            start = tr.endpos;
        }
        temp_entity(
            world,
            TE_BFG_LASER,
            &[origin, tr.endpos],
            None,
            origin,
            Multicast::PHS,
        );
    }
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.nextthink = time + FRAMETIME;
    }
}

/// Warn the monster a player's shot will hit, if it sees it coming, with
/// how long until it lands; on easy only a quarter of the time. C ref:
/// `check_dodge` (g_weapon.c).
fn check_dodge(world: &mut GameWorld, shooter: EntityKey, start: Vec3f, dir: Vec3f, speed: i32) {
    if world.client(shooter).is_none() {
        return; // only players' shots are dodged
    }
    if world.game.skill == SKILL_EASY && world.random() > 0.25 {
        return;
    }
    let end = start + dir * 8192.0;
    let pass = world.number_of(shooter);
    let tr = world
        .gi
        .trace(start, Vec3f::ZERO, Vec3f::ZERO, end, pass, MASK_SHOT);
    let Some(target) = tr.ent_index.and_then(|n| world.entities.key_for_number(n)) else {
        return;
    };
    let Some(t) = world.entities.get(target) else {
        return;
    };
    let dodge = t.monsterinfo.as_ref().and_then(|mi| mi.dodge);
    let alive_monster = t.svflags & SvFlags::MONSTER.bits() != 0 && t.game.health > 0;
    let Some(dodge) = dodge.filter(|_| alive_monster) else {
        return;
    };
    if !crate::ai::infront(world, target, shooter) {
        return;
    }
    let eta = ((tr.endpos - start).length() - t.maxs.x) / speed as f32;
    dodge(world, target, shooter, eta);
}

/// `svc_temp_entity` of `te` with its positions and an optional direction,
/// multicast from `at`.
fn temp_entity(
    world: &GameWorld,
    te: i32,
    positions: &[Vec3f],
    dir: Option<Vec3f>,
    at: Vec3f,
    to: Multicast,
) {
    world.gi.write_byte(SvcOp::TempEntity as i32);
    world.gi.write_byte(te);
    for &p in positions {
        world.gi.write_position(p);
    }
    if let Some(dir) = dir {
        world.gi.write_dir(dir);
    }
    world.gi.multicast(at, to);
}

/// C `8000 / speed`, an int division. C divides by zero on a zero
/// speed; this port gives that projectile 8000 seconds instead.
fn projectile_lifetime(speed: i32) -> f32 {
    8000_i32.checked_div(speed).unwrap_or(8000) as f32
}

/// Timeout cleanup — remove projectile entities that outlived their duration.
fn projectile_timeout(world: &mut GameWorld, self_key: EntityKey) {
    world.free_entity(self_key);
}

// ---------------------------------------------------------------------------
// Weapon fire functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// The C fields every projectile gets: model, facing and clip mask.
    fn launch_projectile(&mut self, key: EntityKey, dir: Vec3f, model: &str) {
        if let Some(ent) = self.entities.get_mut(key) {
            ent.state.angles = vectoangles(dir);
            ent.clipmask = MASK_SHOT;
        }
        self.set_model(key, model);
    }

    /// Fire a blaster bolt. Creates a projectile entity.
    /// C ref: `fire_blaster` (g_weapon.c:447-508).
    ///
    /// `effect`: the bolt's `EF_*` flags (`EF_BLASTER`, `EF_HYPERBLASTER`
    /// or 0). `hyper`: true for hyperblaster variant (different MOD).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_blaster(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        effect: u32,
        hyper: bool,
    ) {
        let dir = dir.normalize_or_zero();
        let Some(bolt_key) = self.spawn() else {
            return;
        };

        if let Some(bolt) = self.entities.get_mut(bolt_key) {
            // C: a dead monster, so player prediction does not clip
            // against bolts.
            bolt.svflags = q2_shared::constants::SVF_DEADMONSTER;
            bolt.state.renderfx |= q2_shared::constants::RF_NOSHADOW;
            bolt.game.classname = "bolt".into();
            bolt.game.movetype = MoveType::FlyMissile;
            bolt.solid = Solid::Bbox;
            bolt.owner = Some(owner);
            bolt.game.dmg = damage;
            bolt.game.spawnflags = if hyper { 1 } else { 0 };
            bolt.state.effects |= effect;

            bolt.state.origin = start;
            bolt.state.old_origin = start;
            bolt.velocity = dir * speed as f32;

            bolt.touch = Some(blaster_touch);
            bolt.think = Some(projectile_timeout);
            bolt.game.nextthink = self.level.time + 2.0;
        }
        self.launch_projectile(bolt_key, dir, "models/objects/laser/tris.md2");
        check_dodge(self, owner, start, dir, speed);

        self.link_entity(bolt_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);

        // A muzzle behind a wall or inside a target hits it at once.
        let Some(owner_origin) = self.entities.get(owner).map(|e| e.state.origin) else {
            return;
        };
        let trace = self.gi.trace(
            owner_origin,
            Vec3f::ZERO,
            Vec3f::ZERO,
            start,
            self.number_of(bolt_key),
            MASK_SHOT,
        );
        if trace.fraction < 1.0 {
            if let Some(bolt) = self.entities.get_mut(bolt_key) {
                bolt.state.origin -= dir * 10.0;
            }
            // C passes tr.ent, which is the world when no entity was hit.
            let other = trace.ent_index.unwrap_or(0);
            if let Some(other) = self.entities.key_for_number(other) {
                blaster_touch(self, bolt_key, other, None, None);
            }
        }
    }

    /// Fire a single bullet (machinegun/chaingun round).
    /// C ref: `fire_bullet` (g_weapon.c:338-349), via `fire_lead`.
    ///
    /// Hitscan — traces immediately, no projectile entity created.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_bullet(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
        hspread: i32,
        vspread: i32,
        means_of_death: MeansOfDeath,
    ) {
        self.fire_lead(
            owner,
            start,
            dir,
            damage,
            kick,
            TE_GUNSHOT,
            hspread,
            vspread,
            means_of_death,
        );
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// One bullet: traced from the muzzle with spread, redirected and
    /// splashing where it enters water, damaging what it hits or puffing
    /// `te_impact` off a wall (not the sky), with a bubble trail through
    /// water. The spread is C's two `crandom()` offsets at 8192 units.
    /// C ref: `fire_lead` (g_weapon.c).
    #[allow(clippy::too_many_arguments)]
    fn fire_lead(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        aimdir: Vec3f,
        damage: i32,
        kick: i32,
        te_impact: i32,
        hspread: i32,
        vspread: i32,
        means_of_death: MeansOfDeath,
    ) {
        let Some(owner_origin) = self.entities.get(owner).map(|e| e.state.origin) else {
            return;
        };
        let pass = self.number_of(owner);
        // A muzzle inside a wall hits the wall, not what is beyond it.
        let mut tr = self.gi.trace(
            owner_origin,
            Vec3f::ZERO,
            Vec3f::ZERO,
            start,
            pass,
            MASK_SHOT,
        );
        let mut water = None;
        if tr.fraction >= 1.0 {
            let (forward, right, up) = angle_vectors(vectoangles(aimdir));
            let r = self.crandom() * hspread as f32;
            let u = self.crandom() * vspread as f32;
            let mut end = start + forward * 8192.0 + right * r + up * u;
            let mut mask = MASK_SHOT | MASK_WATER;
            if self.gi.point_contents(start) & MASK_WATER != 0 {
                water = Some(start);
                mask &= !MASK_WATER;
            }
            tr = self
                .gi
                .trace(start, Vec3f::ZERO, Vec3f::ZERO, end, pass, mask);
            // See if we hit water.
            if tr.contents & MASK_WATER != 0 {
                let water_start = tr.endpos;
                water = Some(water_start);
                if start != tr.endpos {
                    let color = if tr.contents & CONTENTS_WATER != 0 {
                        if tr.surface.as_ref().is_some_and(|s| s.name == "*brwater") {
                            SPLASH_BROWN_WATER
                        } else {
                            SPLASH_BLUE_WATER
                        }
                    } else if tr.contents & CONTENTS_SLIME != 0 {
                        SPLASH_SLIME
                    } else if tr.contents & CONTENTS_LAVA != 0 {
                        SPLASH_LAVA
                    } else {
                        SPLASH_UNKNOWN
                    };
                    if color != SPLASH_UNKNOWN {
                        self.gi.write_byte(SvcOp::TempEntity as i32);
                        self.gi.write_byte(TE_SPLASH);
                        self.gi.write_byte(8);
                        self.gi.write_position(tr.endpos);
                        self.gi.write_dir(tr.plane.normal);
                        self.gi.write_byte(color);
                        self.gi.multicast(tr.endpos, Multicast::PVS);
                    }
                    // Change the bullet's course when it enters water.
                    let (forward, right, up) = angle_vectors(vectoangles(end - start));
                    let r = self.crandom() * hspread as f32 * 2.0;
                    let u = self.crandom() * vspread as f32 * 2.0;
                    end = water_start + forward * 8192.0 + right * r + up * u;
                }
                // Re-trace ignoring water this time.
                tr = self
                    .gi
                    .trace(water_start, Vec3f::ZERO, Vec3f::ZERO, end, pass, MASK_SHOT);
            }
        }

        // Send gun puff / flash.
        let sky = tr.surface.as_ref().is_some_and(|s| s.flags & SURF_SKY != 0);
        if !sky && tr.fraction < 1.0 {
            let hit = tr.ent_index.and_then(|n| self.entities.key_for_number(n));
            let takes_damage = hit
                .and_then(|h| self.entities.get(h))
                .is_some_and(|e| e.game.takedamage != TakeDamage::No);
            match hit {
                Some(hit) if takes_damage => self.t_damage(
                    hit,
                    owner,
                    owner,
                    aimdir,
                    tr.endpos,
                    tr.plane.normal,
                    damage,
                    kick,
                    DamageFlags::BULLET,
                    means_of_death,
                ),
                _ => {
                    let sky_named = tr
                        .surface
                        .as_ref()
                        .is_some_and(|s| s.name.starts_with("sky"));
                    if !sky_named {
                        temp_entity(
                            self,
                            te_impact,
                            &[tr.endpos],
                            Some(tr.plane.normal),
                            tr.endpos,
                            Multicast::PVS,
                        );
                        self.player_noise(owner, tr.endpos, PlayerNoise::Impact);
                    }
                }
            }
        }

        // Through water: a bubble trail from where it went in.
        if let Some(water_start) = water {
            let dir = (tr.endpos - water_start).normalize_or_zero();
            let pos = tr.endpos - dir * 2.0;
            let end = if self.gi.point_contents(pos) & MASK_WATER != 0 {
                pos
            } else {
                self.gi
                    .trace(
                        pos,
                        Vec3f::ZERO,
                        Vec3f::ZERO,
                        water_start,
                        tr.ent_index,
                        MASK_WATER,
                    )
                    .endpos
            };
            let mid = (water_start + end) * 0.5;
            temp_entity(
                self,
                TE_BUBBLETRAIL,
                &[water_start, end],
                None,
                mid,
                Multicast::PVS,
            );
        }
    }

    /// Fire shotgun pellets. Fires `count` individual bullet traces.
    /// C ref: `fire_shotgun` (g_weapon.c:355-371).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_shotgun(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
        hspread: i32,
        vspread: i32,
        count: i32,
        means_of_death: MeansOfDeath,
    ) {
        for _ in 0..count.max(0) {
            self.fire_lead(
                owner,
                start,
                dir,
                damage,
                kick,
                TE_SHOTGUN,
                hspread,
                vspread,
                means_of_death,
            );
        }
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// Fire a rocket. Creates a projectile entity.
    /// C ref: `fire_rocket` (g_weapon.c:829-868).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_rocket(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        damage_radius: f32,
        radius_damage: i32,
    ) {
        let Some(rocket_key) = self.spawn() else {
            return;
        };

        if let Some(rocket) = self.entities.get_mut(rocket_key) {
            rocket.game.classname = "rocket".into();
            rocket.game.movetype = MoveType::FlyMissile;
            rocket.solid = Solid::Bbox;
            rocket.owner = Some(owner);
            rocket.game.dmg = damage;
            rocket.game.radius_dmg = radius_damage;
            rocket.game.dmg_radius = damage_radius;
            rocket.state.effects |= EF_ROCKET;

            rocket.state.origin = start;
            rocket.velocity = dir.normalize_or_zero() * speed as f32;

            rocket.touch = Some(rocket_touch);
            rocket.think = Some(projectile_timeout);
            rocket.game.nextthink = self.level.time + projectile_lifetime(speed);
        }
        self.launch_projectile(rocket_key, dir, "models/objects/rocket/tris.md2");

        check_dodge(self, owner, start, dir, speed);
        self.link_entity(rocket_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// Fire a grenade. Creates a bouncing projectile entity.
    /// C ref: `fire_grenade` (g_weapon.c:646-684). C adds `crandom() * 10`
    /// jitter to the upward and sideways push; there is no game RNG, so
    /// the push is the C mean.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_grenade(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        timer: f32,
        damage_radius: f32,
    ) {
        let Some(gren_key) = self.spawn() else {
            return;
        };

        let (_, right, up) = angle_vectors(vectoangles(dir));
        let lift = 200.0 + self.crandom() * 10.0;
        let side = self.crandom() * 10.0;

        if let Some(gren) = self.entities.get_mut(gren_key) {
            gren.game.classname = "grenade".into();
            gren.game.movetype = MoveType::Bounce;
            gren.solid = Solid::Bbox;
            gren.owner = Some(owner);
            gren.game.dmg = damage;
            gren.game.dmg_radius = damage_radius;
            gren.state.effects |= EF_GRENADE;

            gren.state.origin = start;
            gren.clipmask = MASK_SHOT;
            gren.velocity = dir * speed as f32 + up * lift + right * side;
            // Spin.
            gren.avelocity = Vec3f::new(300.0, 300.0, 300.0);

            gren.touch = Some(grenade_touch);
            gren.think = Some(grenade_think);
            gren.game.nextthink = self.level.time + timer;
        }
        self.launch_projectile(gren_key, dir, "models/objects/grenade/tris.md2");

        self.link_entity(gren_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// A hand grenade: tossed up and to a random side, set to go off after
    /// `timer`, or at once when `timer` has run out (held too long).
    /// `spawnflags` 1 marks a hand grenade, 3 one that went off in the
    /// hand. C ref: `fire_grenade2` (g_weapon.c).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_grenade2(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        aimdir: Vec3f,
        damage: i32,
        speed: i32,
        timer: f32,
        damage_radius: f32,
        held: bool,
    ) {
        let (_, right, up) = angle_vectors(vectoangles(aimdir));
        let lift = 200.0 + self.crandom() * 10.0;
        let side = self.crandom() * 10.0;
        let Some(key) = self.spawn() else {
            return;
        };
        let model = self.gi.model_index("models/objects/grenade2/tris.md2");
        let tick = self.gi.sound_index("weapons/hgrenc1b.wav");
        let nextthink = self.level.time + timer;
        if let Some(gren) = self.entities.get_mut(key) {
            gren.state.origin = start;
            gren.velocity = aimdir * speed as f32 + up * lift + right * side;
            gren.avelocity = Vec3f::new(300.0, 300.0, 300.0);
            gren.game.movetype = MoveType::Bounce;
            gren.clipmask = MASK_SHOT;
            gren.solid = Solid::Bbox;
            gren.state.effects |= EF_GRENADE;
            gren.mins = Vec3f::ZERO;
            gren.maxs = Vec3f::ZERO;
            gren.state.modelindex = model;
            gren.owner = Some(owner);
            gren.touch = Some(grenade_touch);
            gren.game.nextthink = nextthink;
            gren.think = Some(grenade_think);
            gren.game.dmg = damage;
            gren.game.dmg_radius = damage_radius;
            gren.game.classname = "hgrenade".into();
            gren.game.spawnflags = if held { 3 } else { 1 };
            gren.state.sound = tick;
        }
        if timer <= 0.0 {
            grenade_explode(self, key);
        } else {
            let number = self.number_of(owner);
            let throw = self.gi.sound_index("weapons/hgrent1a.wav");
            self.gi
                .sound(number, CHAN_WEAPON, throw, 1.0, ATTN_NORM, 0.0);
            self.link_entity(key);
        }
    }

    /// Fire a railgun shot: an instant line through every monster, player
    /// and bbox entity until something solid, on through slime and lava,
    /// with C's trail (twice when it crossed liquid). C ref: `fire_rail`
    /// (g_weapon.c).
    pub fn fire_rail(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
    ) {
        let end = start + dir * 8192.0;
        let mut from = start;
        let mut ignore = Some(owner);
        let mut water = false;
        let mut mask = MASK_SHOT | CONTENTS_SLIME | CONTENTS_LAVA;
        self.player_noise(owner, start, PlayerNoise::Weapon);
        let mut tr = Trace::default();
        while let Some(pass) = ignore {
            tr = self.gi.trace(
                from,
                Vec3f::ZERO,
                Vec3f::ZERO,
                end,
                self.number_of(pass),
                mask,
            );
            if tr.contents & (CONTENTS_SLIME | CONTENTS_LAVA) != 0 {
                mask &= !(CONTENTS_SLIME | CONTENTS_LAVA);
                water = true;
            } else {
                let hit = tr.ent_index.and_then(|n| self.entities.key_for_number(n));
                let hit_ent = hit.and_then(|h| self.entities.get(h));
                ignore = match hit_ent {
                    Some(e)
                        if e.svflags & SvFlags::MONSTER.bits() != 0
                            || e.client.is_some()
                            || e.solid == Solid::Bbox =>
                    {
                        hit
                    }
                    _ => None,
                };
                let hurt = hit_ent.is_some_and(|e| e.game.takedamage != TakeDamage::No);
                match hit {
                    Some(hit) if hit != owner && hurt => self.t_damage(
                        hit,
                        owner,
                        owner,
                        dir,
                        tr.endpos,
                        tr.plane.normal,
                        damage,
                        kick,
                        DamageFlags::empty(),
                        MeansOfDeath::Railgun,
                    ),
                    _ => ignore = None,
                }
            }
            from = tr.endpos;
        }
        let origin = self.entities.get(owner).map_or(start, |e| e.state.origin);
        // Send gun puff / flash.
        temp_entity(
            self,
            TE_RAILTRAIL,
            &[start, tr.endpos],
            None,
            origin,
            Multicast::PHS,
        );
        if water {
            temp_entity(
                self,
                TE_RAILTRAIL,
                &[start, tr.endpos],
                None,
                tr.endpos,
                Multicast::PHS,
            );
        }
        self.player_noise(owner, tr.endpos, PlayerNoise::Impact);
    }

    /// Fire a BFG projectile. C ref: `fire_bfg` (g_weapon.c:1188-1231),
    /// without the laser think (the blast flies until it hits or times out).
    pub fn fire_bfg(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        speed: i32,
        damage_radius: f32,
    ) {
        let Some(bfg_key) = self.spawn() else {
            return;
        };

        if let Some(bfg) = self.entities.get_mut(bfg_key) {
            bfg.game.classname = "bfg blast".into();
            bfg.game.movetype = MoveType::FlyMissile;
            bfg.solid = Solid::Bbox;
            bfg.owner = Some(owner);
            bfg.game.dmg = 200; // Core explosion damage is hardcoded.
            bfg.game.radius_dmg = damage;
            bfg.game.dmg_radius = damage_radius;
            bfg.state.effects |= EF_BFG | EF_ANIM_ALLFAST;

            bfg.state.origin = start;
            bfg.velocity = dir.normalize_or_zero() * speed as f32;

            bfg.mins = Vec3f::ZERO;
            bfg.maxs = Vec3f::ZERO;
            bfg.touch = Some(bfg_touch);
            // C sets G_FreeEdict at 8000 / speed, then replaces it with
            // bfg_think: the ball flies until it hits something.
            bfg.think = Some(bfg_think);
            bfg.game.nextthink = self.level.time + FRAMETIME;
        }
        let hum = self.gi.sound_index("weapons/bfg__l1a.wav");
        if let Some(bfg) = self.entities.get_mut(bfg_key) {
            bfg.state.sound = hum;
        }
        self.launch_projectile(bfg_key, dir, "sprites/s_bfg1.sp2");
        check_dodge(self, owner, start, dir, speed);

        self.link_entity(bfg_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// Melee attack on `owner`'s enemy. `aim` is (range, right offset, up
    /// offset). Returns whether a monster or client was hit.
    /// C ref: `fire_hit` (g_weapon.c:71-167).
    pub fn fire_hit(&mut self, owner: EntityKey, aim: Vec3f, damage: i32, kick: i32) -> bool {
        let Some(me) = self.entities.get(owner) else {
            return false;
        };
        let (origin, angles, my_mins, my_maxs) =
            (me.state.origin, me.state.angles, me.mins, me.maxs);
        let Some(enemy) = me.game.enemy else {
            return false;
        };
        let Some(en) = self.entities.get(enemy) else {
            return false;
        };
        let (enemy_origin, enemy_mins, enemy_maxs) = (en.state.origin, en.mins, en.maxs);

        let dir = enemy_origin - origin;
        let mut range = dir.length();
        if range > aim.x {
            return false;
        }
        let mut aim = aim;
        if aim.y > my_mins.x && aim.y < my_maxs.x {
            // Straight on: back the range up to the edge of their box.
            range -= enemy_maxs.x;
        } else {
            // Side hit: move the right offset out to the edge of their box.
            aim.y = if aim.y < 0.0 {
                enemy_mins.x
            } else {
                enemy_maxs.x
            };
        }

        let point = origin + dir * range;
        let trace = self.gi.trace(
            origin,
            Vec3f::ZERO,
            Vec3f::ZERO,
            point,
            self.number_of(owner),
            MASK_SHOT,
        );
        // C: an unobstructed trace reports the world, which takes no damage.
        let mut hit = None;
        if trace.fraction < 1.0 {
            let Some(target) = trace
                .ent_index
                .and_then(|n| self.entities.key_for_number(n))
            else {
                return false;
            };
            let Some(t) = self.entities.get(target) else {
                return false;
            };
            if t.game.takedamage == TakeDamage::No {
                return false;
            }
            // Hitting any client or monster hits the one we wanted.
            hit = Some(
                if t.svflags & SvFlags::MONSTER.bits() != 0 || t.client.is_some() {
                    enemy
                } else {
                    target
                },
            );
        }

        let (forward, right, up) = angle_vectors(angles);
        let point = origin + forward * range + right * aim.y + up * aim.z;
        let Some(hit) = hit else {
            return false;
        };
        self.t_damage(
            hit,
            owner,
            owner,
            point - enemy_origin,
            point,
            Vec3f::ZERO,
            damage,
            kick / 2,
            DamageFlags::NO_KNOCKBACK,
            MeansOfDeath::Hit,
        );
        let Some(h) = self.entities.get(hit) else {
            return false;
        };
        if h.svflags & SvFlags::MONSTER.bits() == 0 && h.client.is_none() {
            return false;
        }

        // Our special form of knockback.
        let Some(en) = self.entities.get_mut(enemy) else {
            return false;
        };
        let center = en.absmin + en.size * 0.5;
        en.velocity += (center - point).normalize_or_zero() * kick as f32;
        if en.velocity.z > 0.0 {
            en.game.ground_entity = None;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// C `vectoangles` (g_utils.c), including its truncation to whole degrees.
pub(crate) fn vectoangles(v: Vec3f) -> Vec3f {
    let (yaw, pitch) = if v.x == 0.0 && v.y == 0.0 {
        (0.0, if v.z > 0.0 { 90.0 } else { 270.0 })
    } else {
        let mut yaw = if v.x != 0.0 {
            (v.y.atan2(v.x).to_degrees() as i32) as f32
        } else if v.y > 0.0 {
            90.0
        } else {
            -90.0
        };
        if yaw < 0.0 {
            yaw += 360.0;
        }
        let forward = (v.x * v.x + v.y * v.y).sqrt();
        let mut pitch = (v.z.atan2(forward).to_degrees() as i32) as f32;
        if pitch < 0.0 {
            pitch += 360.0;
        }
        (yaw, pitch)
    };
    Vec3f::new(-pitch, yaw, 0.0)
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    // ---- check_dodge ----

    fn note_dodge(world: &mut GameWorld, key: EntityKey, _attacker: EntityKey, eta: f32) {
        world.entities.get_mut(key).unwrap().game.wait = eta;
    }

    /// A player at the origin facing +x and a dodging monster at x = 300
    /// facing back, on the bolt's line.
    fn dodge_range() -> (
        GameWorld,
        EntityKey,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        let player = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(player);
        let monster = world.spawn().unwrap();
        let m = world.entities.get_mut(monster).unwrap();
        m.state.origin = Vec3f::new(300.0, 0.0, 0.0);
        m.state.angles = Vec3f::new(0.0, 180.0, 0.0);
        m.maxs = Vec3f::splat(16.0);
        m.game.health = 50;
        m.svflags |= SvFlags::MONSTER.bits();
        m.monsterinfo = Some(Box::default());
        m.monsterinfo.as_mut().unwrap().dodge = Some(note_dodge);
        world.entities.get_mut(player).unwrap().state.origin = Vec3f::ZERO;
        let number = world.number_of(monster).unwrap();
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.03,
            endpos: Vec3f::new(284.0, 0.0, 0.0),
            ent_index: Some(number),
            ..Default::default()
        });
        (world, player, monster, gi)
    }

    #[test]
    fn a_player_shot_warns_the_monster_it_will_hit_with_its_eta() {
        let (mut world, player, monster, _) = dodge_range();
        world.game.skill = 1;
        world.fire_blaster(player, Vec3f::ZERO, Vec3f::X, 10, 1000, EF_BLASTER, false);
        // (284 - 16) / 1000.
        let eta = world.entities.get(monster).unwrap().game.wait;
        assert!((eta - 0.268).abs() < 1e-4, "{eta}");
    }

    #[test]
    fn monsters_firing_do_not_trigger_dodges_and_the_dead_do_not_dodge() {
        let (mut world, _, monster, _) = dodge_range();
        world.game.skill = 1;
        world.fire_blaster(monster, Vec3f::ZERO, Vec3f::X, 10, 1000, EF_BLASTER, false);
        assert_eq!(
            world.entities.get(monster).unwrap().game.wait,
            0.0,
            "monster shot"
        );

        let (mut world, player, monster, _) = dodge_range();
        world.game.skill = 1;
        world.entities.get_mut(monster).unwrap().game.health = 0;
        world.fire_rocket(player, Vec3f::ZERO, Vec3f::X, 100, 650, 120.0, 120);
        assert_eq!(world.entities.get(monster).unwrap().game.wait, 0.0, "dead");
    }

    #[test]
    fn a_monster_facing_away_does_not_see_the_shot_coming() {
        let (mut world, player, monster, _) = dodge_range();
        world.game.skill = 1;
        world.entities.get_mut(monster).unwrap().state.angles.y = 0.0;
        world.fire_bfg(player, Vec3f::ZERO, Vec3f::X, 500, 400, 1000.0);
        assert_eq!(world.entities.get(monster).unwrap().game.wait, 0.0);
    }

    // ---- Temp entities (C's svc_temp_entity writes) ----

    use q2_shared::constants::*;
    use q2_shared::protocol::SvcOp;
    use q2_shared::types::Multicast;

    /// The temp entity types written so far, in order.
    /// Each multicast message's first two bytes are svc_temp_entity and
    /// the type; read them per message, since a type can equal the op.
    fn temp_entities(gi: &crate::world::MockGameImport) -> Vec<i32> {
        let writes = gi.writes.borrow();
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < writes.len() {
            if writes[i] == SvcOp::TempEntity as i32 {
                out.push(writes[i + 1]);
                i += 2;
            } else {
                i += 1;
            }
        }
        out
    }

    fn wall_hit(endpos: Vec3f, surface: &str, flags: i32) -> Trace {
        Trace {
            fraction: 0.5,
            endpos,
            plane: Plane {
                normal: Vec3f::new(-1.0, 0.0, 0.0),
                ..Default::default()
            },
            surface: Some(Surface {
                name: surface.into(),
                flags,
                value: 0,
            }),
            ent_index: Some(0),
            ..Default::default()
        }
    }

    fn shooter() -> (
        GameWorld,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.entities.spawn_numbered(0).unwrap();
        let owner = world.spawn().unwrap();
        (world, owner, gi)
    }

    #[test]
    fn a_bullet_into_a_wall_puffs_a_gunshot_and_a_pellet_a_shotgun_puff() {
        let (mut world, owner, gi) = shooter();
        let hit = Vec3f::new(100.0, 0.0, 0.0);
        gi.queued_traces
            .borrow_mut()
            .extend([Trace::default(), wall_hit(hit, "e1u1/wall", 0)]);
        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            8,
            2,
            0,
            0,
            MeansOfDeath::Machinegun,
        );
        assert_eq!(temp_entities(&gi), [TE_GUNSHOT]);
        assert_eq!(*gi.vectors.borrow(), [hit, Vec3f::new(-1.0, 0.0, 0.0)]);
        assert_eq!(
            gi.multicasts.borrow().last().unwrap(),
            &(hit, Multicast::PVS)
        );

        let (mut world, owner, gi) = shooter();
        gi.queued_traces
            .borrow_mut()
            .extend([Trace::default(), wall_hit(hit, "e1u1/wall", 0)]);
        world.fire_shotgun(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            4,
            8,
            0,
            0,
            1,
            MeansOfDeath::Shotgun,
        );
        assert_eq!(temp_entities(&gi), [TE_SHOTGUN]);
    }

    #[test]
    fn bullets_into_the_sky_leave_no_puff() {
        let (mut world, owner, gi) = shooter();
        gi.queued_traces
            .borrow_mut()
            .extend([Trace::default(), wall_hit(Vec3f::X, "sky1", SURF_SKY)]);
        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            8,
            2,
            0,
            0,
            MeansOfDeath::Machinegun,
        );
        let (mut world2, owner2, gi2) = shooter();
        gi2.queued_traces
            .borrow_mut()
            .extend([Trace::default(), wall_hit(Vec3f::X, "sky2", 0)]);
        world2.fire_bullet(
            owner2,
            Vec3f::ZERO,
            Vec3f::X,
            8,
            2,
            0,
            0,
            MeansOfDeath::Machinegun,
        );
        assert!(temp_entities(&gi).is_empty());
        assert!(temp_entities(&gi2).is_empty(), "a sky-named surface too");
    }

    #[test]
    fn a_bullet_entering_water_splashes_and_leaves_a_bubble_trail() {
        let (mut world, owner, gi) = shooter();
        let surface = Vec3f::new(50.0, 0.0, 0.0);
        let bottom = Vec3f::new(120.0, 0.0, 0.0);
        let water = Trace {
            fraction: 0.3,
            endpos: surface,
            contents: CONTENTS_WATER,
            surface: Some(Surface {
                name: "e1u1/water".into(),
                flags: 0,
                value: 0,
            }),
            plane: Plane {
                normal: Vec3f::Z,
                ..Default::default()
            },
            ..Default::default()
        };
        gi.queued_traces.borrow_mut().extend([
            Trace::default(),
            water,
            wall_hit(bottom, "e1u1/floor", 0),
        ]);
        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            8,
            2,
            0,
            0,
            MeansOfDeath::Machinegun,
        );
        assert_eq!(temp_entities(&gi), [TE_SPLASH, TE_GUNSHOT, TE_BUBBLETRAIL]);
        let writes = gi.writes.borrow();
        let at = writes.iter().position(|&w| w == TE_SPLASH).unwrap();
        assert_eq!((writes[at + 1], writes[at + 2]), (8, SPLASH_BLUE_WATER));
    }

    #[test]
    fn hitting_something_that_bleeds_sprays_blood_not_a_puff() {
        let (mut world, owner, gi) = shooter();
        let monster = world.spawn().unwrap();
        let m = world.entities.get_mut(monster).unwrap();
        m.game.takedamage = TakeDamage::Yes;
        m.game.health = 100;
        m.svflags |= SvFlags::MONSTER.bits();
        let number = world.number_of(monster).unwrap();
        let mut hit = wall_hit(Vec3f::X, "", 0);
        hit.ent_index = Some(number);
        gi.queued_traces
            .borrow_mut()
            .extend([Trace::default(), hit]);
        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            8,
            2,
            0,
            0,
            MeansOfDeath::Machinegun,
        );
        assert_eq!(temp_entities(&gi), [TE_BLOOD]);
    }

    #[test]
    fn a_blaster_bolt_flashes_on_walls_and_vanishes_in_the_sky() {
        let (mut world, owner, gi) = shooter();
        world.fire_blaster(owner, Vec3f::ZERO, Vec3f::X, 10, 1000, EF_BLASTER, false);
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        let wall = world.entities.key_for_number(0).unwrap();
        let plane = Plane {
            normal: Vec3f::Z,
            ..Default::default()
        };
        blaster_touch(&mut world, bolt, wall, Some(&plane), None);
        assert_eq!(temp_entities(&gi), [TE_BLASTER]);
        assert!(!world.entities.get(bolt).is_some_and(|e| e.in_use));

        let (mut world, owner, gi) = shooter();
        world.fire_blaster(owner, Vec3f::ZERO, Vec3f::X, 10, 1000, EF_BLASTER, false);
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        let wall = world.entities.key_for_number(0).unwrap();
        let sky = Surface {
            name: "sky".into(),
            flags: SURF_SKY,
            value: 0,
        };
        blaster_touch(&mut world, bolt, wall, None, Some(&sky));
        assert!(temp_entities(&gi).is_empty());
        assert!(!world.entities.get(bolt).is_some_and(|e| e.in_use));
    }

    #[test]
    fn rockets_explode_with_a_temp_entity_heard_through_the_phs() {
        let (mut world, owner, gi) = shooter();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 120.0, 120);
        let rocket = world.find_by_classname(None, "rocket").unwrap();
        let wall = world.entities.key_for_number(0).unwrap();
        rocket_touch(&mut world, rocket, wall, None, None);
        assert_eq!(temp_entities(&gi), [TE_ROCKET_EXPLOSION]);
        // Placed 0.02 s back along the flight, so it shows off the wall.
        assert_eq!(gi.vectors.borrow()[0], Vec3f::new(-13.0, 0.0, 0.0));
        assert_eq!(gi.multicasts.borrow().last().unwrap().1, Multicast::PHS);

        let (mut world, owner, gi) = shooter();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 120.0, 120);
        let rocket = world.find_by_classname(None, "rocket").unwrap();
        world.entities.get_mut(rocket).unwrap().game.waterlevel = 1;
        let wall = world.entities.key_for_number(0).unwrap();
        rocket_touch(&mut world, rocket, wall, None, None);
        assert_eq!(temp_entities(&gi), [TE_ROCKET_EXPLOSION_WATER]);
    }

    #[test]
    fn single_player_rockets_knock_debris_off_solid_walls() {
        let (mut world, owner, _) = shooter();
        for _ in 0..8 {
            world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 120.0, 120);
            let rocket = world.find_by_classname(None, "rocket").unwrap();
            let wall = world.entities.key_for_number(0).unwrap();
            let solid = Surface {
                name: "e1u1/wall".into(),
                flags: 0,
                value: 0,
            };
            rocket_touch(&mut world, rocket, wall, None, Some(&solid));
        }
        assert!(
            world.find_by_classname(None, "debris").is_some(),
            "randk() % 5 chunks"
        );
    }

    #[test]
    fn grenades_explode_as_grenade_blasts_on_the_ground_and_rocket_blasts_in_the_air() {
        for (ground, water, want) in [
            (true, false, TE_GRENADE_EXPLOSION),
            (false, false, TE_ROCKET_EXPLOSION),
            (true, true, TE_GRENADE_EXPLOSION_WATER),
            (false, true, TE_ROCKET_EXPLOSION_WATER),
        ] {
            let (mut world, owner, gi) = shooter();
            world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 160.0);
            let gren = world.find_by_classname(None, "grenade").unwrap();
            let wall = world.entities.key_for_number(0);
            let g = world.entities.get_mut(gren).unwrap();
            g.game.ground_entity = if ground { wall } else { None };
            g.game.waterlevel = i32::from(water);
            grenade_explode(&mut world, gren);
            assert_eq!(temp_entities(&gi), [want]);
        }
    }

    #[test]
    fn a_rail_leaves_a_trail_from_the_muzzle_to_where_it_stopped() {
        let (mut world, owner, gi) = shooter();
        let end = Vec3f::new(500.0, 0.0, 0.0);
        gi.queued_traces
            .borrow_mut()
            .push_back(wall_hit(end, "e1u1/wall", 0));
        world.fire_rail(owner, Vec3f::new(1.0, 2.0, 3.0), Vec3f::X, 100, 200);
        assert_eq!(temp_entities(&gi), [TE_RAILTRAIL]);
        assert_eq!(*gi.vectors.borrow(), [Vec3f::new(1.0, 2.0, 3.0), end]);
        assert_eq!(gi.multicasts.borrow().last().unwrap().1, Multicast::PHS);
    }

    #[test]
    fn a_rail_through_slime_traces_on_and_sends_a_second_trail() {
        let (mut world, owner, gi) = shooter();
        let slime = Trace {
            fraction: 0.2,
            endpos: Vec3f::new(100.0, 0.0, 0.0),
            contents: CONTENTS_SLIME,
            ..Default::default()
        };
        gi.queued_traces
            .borrow_mut()
            .extend([slime, wall_hit(Vec3f::new(400.0, 0.0, 0.0), "e1u1/wall", 0)]);
        world.fire_rail(owner, Vec3f::ZERO, Vec3f::X, 100, 200);
        assert_eq!(temp_entities(&gi), [TE_RAILTRAIL, TE_RAILTRAIL]);
        assert_eq!(
            gi.trace_masks.borrow()[1],
            MASK_SHOT,
            "slime dropped from the mask"
        );
    }

    #[test]
    fn the_bfg_ball_hums_and_lasers_nearby_monsters_each_frame() {
        let (mut world, owner, gi) = shooter();
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 500, 400, 1000.0);
        let ball = world.find_by_classname(None, "bfg blast").unwrap();
        let e = world.entities.get(ball).unwrap();
        assert_eq!(e.state.sound, world.gi.sound_index("weapons/bfg__l1a.wav"));
        assert!(std::ptr::fn_addr_eq(
            e.think.unwrap(),
            bfg_think as crate::entity::ThinkFn
        ));

        let monster = world.spawn().unwrap();
        let m = world.entities.get_mut(monster).unwrap();
        m.state.origin = Vec3f::new(100.0, 0.0, 0.0);
        m.game.takedamage = TakeDamage::Yes;
        m.game.health = 100;
        m.svflags |= SvFlags::MONSTER.bits();
        m.solid = Solid::Bbox; // findradius skips SOLID_NOT
        world.link_entity(monster);
        *gi.box_edicts.borrow_mut() = vec![world.number_of(monster).unwrap()];
        bfg_think(&mut world, ball);
        assert!(temp_entities(&gi).contains(&TE_BFG_LASER));
    }

    #[test]
    fn a_bfg_ball_bursts_into_its_explosion_frames_then_goes() {
        let (mut world, owner, gi) = shooter();
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 500, 400, 1000.0);
        let ball = world.find_by_classname(None, "bfg blast").unwrap();
        let wall = world.entities.key_for_number(0).unwrap();
        bfg_touch(&mut world, ball, wall, None, None);
        assert_eq!(temp_entities(&gi), [TE_BFG_BIGEXPLOSION]);
        let e = world.entities.get(ball).unwrap();
        assert!(e.in_use && e.touch.is_none() && e.velocity == Vec3f::ZERO);
        assert_eq!(
            e.state.modelindex,
            world.gi.model_index("sprites/s_bfg3.sp2")
        );
        for _ in 0..5 {
            let think = world.entities.get(ball).unwrap().think.unwrap();
            think(&mut world, ball);
        }
        assert_eq!(world.entities.get(ball).unwrap().state.frame, 5);
        let think = world.entities.get(ball).unwrap().think.unwrap();
        think(&mut world, ball);
        assert!(
            !world.entities.get(ball).is_some_and(|e| e.in_use),
            "freed after frame 4"
        );
    }

    #[test]
    fn fire_blaster_spawns_bolt() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        assert_eq!(world.entities.count(), 1);

        world.fire_blaster(
            owner,
            Vec3f::new(0.0, 0.0, 0.0),
            Vec3f::new(1.0, 0.0, 0.0),
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        assert_eq!(world.entities.count(), 2);

        // Find the bolt.
        let bolt_key = world.find_by_classname(None, "bolt").unwrap();
        let bolt = world.entities.get(bolt_key).unwrap();
        assert_eq!(bolt.game.classname, "bolt");
        assert_eq!(bolt.game.movetype, MoveType::FlyMissile);
        assert_eq!(bolt.game.dmg, 15);
        assert!((bolt.velocity.x - 1000.0).abs() < 1.0);
        assert!(bolt.touch.is_some());
        assert!(bolt.think.is_some());
        assert_eq!(bolt.owner, Some(owner));
    }

    #[test]
    fn fire_blaster_hyper_sets_spawnflag() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            20,
            1000,
            q2_shared::constants::EF_BLASTER,
            true,
        );

        let bolt_key = world.find_by_classname(None, "bolt").unwrap();
        let bolt = world.entities.get(bolt_key).unwrap();
        assert_eq!(bolt.game.spawnflags, 1);
    }

    #[test]
    fn projectiles_carry_their_c_light_and_trail_effects() {
        use q2_shared::constants::{
            EF_ANIM_ALLFAST, EF_BFG, EF_GRENADE, EF_HYPERBLASTER, EF_ROCKET,
        };
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_blaster(
            owner,
            Vec3f::new(8.0, 0.0, 0.0),
            Vec3f::X,
            15,
            1000,
            EF_HYPERBLASTER,
            true,
        );
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);
        let effects = |class: &str| {
            let key = world.find_by_classname(None, class).unwrap();
            world.entities.get(key).unwrap().state.effects
        };
        assert_eq!(effects("bolt"), EF_HYPERBLASTER);
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        // C fire_blaster starts the bolt's lerp at the muzzle.
        assert_eq!(
            world.entities.get(bolt).unwrap().state.old_origin,
            Vec3f::new(8.0, 0.0, 0.0)
        );
        assert_eq!(effects("rocket"), EF_ROCKET);
        assert_eq!(effects("grenade"), EF_GRENADE);
        assert_eq!(effects("bfg blast"), EF_BFG | EF_ANIM_ALLFAST);
    }

    #[test]
    fn fire_rocket_spawns_rocket() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);

        let rocket_key = world.find_by_classname(None, "rocket").unwrap();
        let rocket = world.entities.get(rocket_key).unwrap();
        assert_eq!(rocket.game.classname, "rocket");
        assert_eq!(rocket.game.movetype, MoveType::FlyMissile);
        assert_eq!(rocket.game.dmg, 100);
        assert_eq!(rocket.game.radius_dmg, 120);
        assert!((rocket.game.dmg_radius - 150.0).abs() < 0.01);
        assert!((rocket.velocity.x - 650.0).abs() < 1.0);
    }

    #[test]
    fn fire_grenade_spawns_bouncing_projectile() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);

        let gren_key = world.find_by_classname(None, "grenade").unwrap();
        let gren = world.entities.get(gren_key).unwrap();
        assert_eq!(gren.game.movetype, MoveType::Bounce);
        assert_eq!(gren.game.dmg, 120);
        // Forward velocity + upward bias.
        assert!(gren.velocity.x > 0.0);
        assert!(gren.velocity.z > 0.0);
        // Spin.
        assert_eq!(gren.avelocity, Vec3f::new(300.0, 300.0, 300.0));
    }

    #[test]
    fn fire_bfg_spawns_projectile() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);

        let bfg_key = world.find_by_classname(None, "bfg blast").unwrap();
        let bfg = world.entities.get(bfg_key).unwrap();
        assert_eq!(bfg.game.dmg, 200); // Hardcoded core damage.
        assert_eq!(bfg.game.radius_dmg, 200);
        assert!((bfg.game.dmg_radius - 1000.0).abs() < 0.01);
    }

    #[test]
    fn blaster_touch_damages_target() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        let target = world.spawn().unwrap();

        // Create bolt.
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        let bolt_key = world.find_by_classname(None, "bolt").unwrap();

        // Set up target.
        {
            let targ = world.entities.get_mut(target).unwrap();
            targ.game.health = 100;
            targ.game.takedamage = TakeDamage::Yes;
            targ.solid = Solid::Bbox;
        }

        // Simulate touch.
        blaster_touch(&mut world, bolt_key, target, None, None);

        // Target should have taken damage.
        assert_eq!(world.entities.get(target).unwrap().game.health, 85);
        // Bolt should be freed.
        assert!(!world.entities.get(bolt_key).is_some_and(|e| e.in_use));
    }

    #[test]
    fn blaster_touch_ignores_owner() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        let bolt_key = world.find_by_classname(None, "bolt").unwrap();

        {
            let ent = world.entities.get_mut(owner).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
        }

        // Touch with owner — should not damage.
        blaster_touch(&mut world, bolt_key, owner, None, None);

        assert_eq!(world.entities.get(owner).unwrap().game.health, 100);
        // Bolt should still exist (touch was ignored).
        assert!(world.entities.get(bolt_key).is_some());
    }

    #[test]
    fn rocket_touch_applies_radius_damage() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        let target = world.spawn().unwrap();
        let bystander = world.spawn().unwrap();

        // Fire rocket.
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        let rocket_key = world.find_by_classname(None, "rocket").unwrap();

        // Set up target and bystander.
        {
            let targ = world.entities.get_mut(target).unwrap();
            targ.state.origin = Vec3f::new(10.0, 0.0, 0.0);
            targ.game.health = 200;
            targ.game.takedamage = TakeDamage::Yes;
            targ.solid = Solid::Bbox;
        }
        {
            let by = world.entities.get_mut(bystander).unwrap();
            by.state.origin = Vec3f::new(50.0, 0.0, 0.0);
            by.game.health = 200;
            by.game.takedamage = TakeDamage::Yes;
            by.solid = Solid::Bbox;
        }

        // Simulate rocket hitting target.
        rocket_touch(&mut world, rocket_key, target, None, None);

        // Target takes direct damage (100).
        assert!(world.entities.get(target).unwrap().game.health < 200);
        // Bystander takes splash damage (within 150 radius).
        assert!(world.entities.get(bystander).unwrap().game.health < 200);
        // Rocket is freed.
        assert!(!world.entities.get(rocket_key).is_some_and(|e| e.in_use));
    }

    #[test]
    fn grenade_explode_does_radius_damage() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        let target = world.spawn().unwrap();

        // Manually create a grenade.
        let gren = world.spawn().unwrap();
        {
            let g = world.entities.get_mut(gren).unwrap();
            g.game.classname = "grenade".into();
            g.game.dmg = 120;
            g.game.dmg_radius = 150.0;
            g.owner = Some(owner);
            g.state.origin = Vec3f::ZERO;
        }
        {
            let t = world.entities.get_mut(target).unwrap();
            t.state.origin = Vec3f::new(30.0, 0.0, 0.0);
            t.game.health = 200;
            t.game.takedamage = TakeDamage::Yes;
            t.solid = Solid::Bbox;
        }

        grenade_explode(&mut world, gren);

        // Target should have taken splash damage.
        assert!(world.entities.get(target).unwrap().game.health < 200);
        // Grenade should be freed.
        assert!(!world.entities.get(gren).is_some_and(|e| e.in_use));
    }

    /// A grenade with `spawnflags` at the origin, a target 30 units away
    /// and, with `direct`, the target as the one it hit.
    fn grenade_blast(spawnflags: i32, direct: bool) -> GameWorld {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        let target = world.spawn().unwrap();
        let gren = world.spawn().unwrap();
        let g = world.entities.get_mut(gren).unwrap();
        g.game.dmg = 120;
        g.game.dmg_radius = 150.0;
        g.game.spawnflags = spawnflags;
        g.owner = Some(owner);
        g.game.enemy = direct.then_some(target);
        let t = world.entities.get_mut(target).unwrap();
        t.state.origin = Vec3f::new(30.0, 0.0, 0.0);
        t.game.health = 200;
        t.game.takedamage = TakeDamage::Yes;
        t.solid = Solid::Bbox;
        grenade_explode(&mut world, gren);
        world
    }

    #[test]
    fn grenade_blasts_record_c_means_of_death_by_kind() {
        // The splash is the last T_Damage, so it is what meansOfDeath holds.
        let splash = |flags| grenade_blast(flags, false).means_of_death;
        assert_eq!(splash(0), MeansOfDeath::GrenadeSplash);
        assert_eq!(splash(1), MeansOfDeath::HandgrenadeSplash);
        assert_eq!(splash(3), MeansOfDeath::HeldGrenade);
    }

    #[test]
    fn a_direct_grenade_hit_is_recorded_before_the_splash() {
        // The splash leaves out the entity hit, so with nothing else in
        // range the direct hit's kind stays recorded.
        assert_eq!(
            grenade_blast(1, true).means_of_death,
            MeansOfDeath::Handgrenade
        );
        assert_eq!(grenade_blast(0, true).means_of_death, MeansOfDeath::Grenade);
    }

    #[test]
    fn t_damage_records_the_means_of_death_like_c_meansofdeath() {
        let mut world = test_world();
        let target = world.spawn().unwrap();
        let t = world.entities.get_mut(target).unwrap();
        t.game.health = 100;
        t.game.takedamage = TakeDamage::Yes;
        world.t_damage(
            target,
            target,
            target,
            Vec3f::X,
            Vec3f::ZERO,
            Vec3f::ZERO,
            10,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Railgun,
        );
        assert_eq!(world.means_of_death, MeansOfDeath::Railgun);

        // A target that can't be hurt leaves it alone (C returns first).
        let wall = world.spawn().unwrap();
        world.t_damage(
            wall,
            wall,
            wall,
            Vec3f::X,
            Vec3f::ZERO,
            Vec3f::ZERO,
            10,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Blaster,
        );
        assert_eq!(world.means_of_death, MeansOfDeath::Railgun);
    }

    /// Bounce a grenade off the world (no `takedamage`) and return the
    /// sound it played.
    fn bounce_sound(spawnflags: i32, seed_draws: usize) -> String {
        let (mut world, gi) = crate::world::test_world_recording();
        for _ in 0..seed_draws {
            world.random();
        }
        let owner = world.spawn().unwrap();
        let gren = world.spawn().unwrap();
        world.entities.get_mut(gren).unwrap().owner = Some(owner);
        world.entities.get_mut(gren).unwrap().game.spawnflags = spawnflags;
        let wall = world.spawn().unwrap(); // takedamage No
        grenade_touch(&mut world, gren, wall, None, None);
        assert!(
            world.entities.get(gren).unwrap().in_use,
            "bounced, not exploded"
        );
        let calls = gi.sound_calls.borrow().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, CHAN_VOICE);
        let name = gi.sounds.borrow()[calls[0].2 as usize - 1].clone();
        name
    }

    #[test]
    fn a_launcher_grenade_bounces_with_grenlb1b() {
        assert_eq!(bounce_sound(0, 0), "weapons/grenlb1b.wav");
    }

    #[test]
    fn a_hand_grenade_bounces_with_either_of_two_sounds() {
        let heard: std::collections::HashSet<String> =
            (0..16).map(|draws| bounce_sound(1, draws)).collect();
        let expected: std::collections::HashSet<String> =
            ["weapons/hgrenb1a.wav", "weapons/hgrenb2a.wav"]
                .into_iter()
                .map(String::from)
                .collect();
        assert_eq!(heard, expected);
    }

    #[test]
    fn launcher_grenades_lift_and_drift_by_c_random_amounts() {
        // C: velocity = dir * speed + (200 + crandom()*10) up
        // + crandom()*10 right. Fired along +x: up is +z, right is -y.
        let mut lifts = Vec::new();
        for draws in 0..8 {
            let mut world = test_world();
            for _ in 0..draws {
                world.random();
            }
            let owner = world.spawn().unwrap();
            world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 160.0);
            let gren = world.find_by_classname(None, "grenade").unwrap();
            let v = world.entities.get(gren).unwrap().velocity;
            assert!((v.x - 600.0).abs() < 1e-3, "{v:?}");
            assert!((190.0..=210.0).contains(&v.z), "{v:?}");
            assert!(v.y.abs() <= 10.0, "{v:?}");
            assert_eq!(world.entities.get(gren).unwrap().clipmask, MASK_SHOT);
            lifts.push(v.z);
        }
        assert!(lifts.iter().any(|&z| z != lifts[0]), "random: {lifts:?}");
    }

    #[test]
    fn grenade_touch_explodes_on_damageable() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        let gren_key = world.find_by_classname(None, "grenade").unwrap();

        let target = world.spawn().unwrap();
        {
            let t = world.entities.get_mut(target).unwrap();
            t.state.origin = Vec3f::new(5.0, 0.0, 0.0);
            t.game.health = 200;
            t.game.takedamage = TakeDamage::Yes;
            t.solid = Solid::Bbox;
        }

        grenade_touch(&mut world, gren_key, target, None, None);

        // Target should have taken damage.
        assert!(world.entities.get(target).unwrap().game.health < 200);
        // Grenade should be freed after explosion.
        assert!(!world.entities.get(gren_key).is_some_and(|e| e.in_use));
    }

    #[test]
    fn fire_shotgun_fires_multiple_pellets() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();

        // Shotgun fires 12 pellets. Each calls fire_bullet (hitscan, no entities).
        world.fire_shotgun(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            4,  // damage per pellet
            10, // kick
            500,
            500, // spread
            12,  // count
            MeansOfDeath::Shotgun,
        );

        // No projectile entities should be created (hitscan).
        assert_eq!(world.entities.count(), 1); // Only the owner.
    }

    #[test]
    fn projectile_timeout_frees_entity() {
        let mut world = test_world();
        let ent = world.spawn().unwrap();

        projectile_timeout(&mut world, ent);

        assert!(!world.entities.get(ent).is_some_and(|e| e.in_use));
    }

    fn target_at(world: &mut GameWorld, origin: Vec3f, health: i32) -> EntityKey {
        let key = world.spawn().unwrap();
        let t = world.entities.get_mut(key).unwrap();
        t.state.origin = origin;
        t.game.health = health;
        t.game.takedamage = TakeDamage::Yes;
        t.solid = Solid::Bbox;
        key
    }

    fn hit(world: &GameWorld, gi: &crate::world::MockGameImport, target: EntityKey) {
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            endpos: world.entities.get(target).unwrap().state.origin,
            ent_index: world.number_of(target),
            ..Trace::default()
        });
    }

    #[test]
    fn projectiles_clip_against_monsters_with_c_mask_shot() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);
        for class in ["bolt", "rocket", "grenade", "bfg blast"] {
            let key = world.find_by_classname(None, class).unwrap();
            // C MASK_SHOT: SOLID | WINDOW | MONSTER | DEADMONSTER.
            assert_eq!(
                world.entities.get(key).unwrap().clipmask,
                0x0600_0003,
                "{class}"
            );
        }
    }

    #[test]
    fn projectiles_carry_their_c_world_model_and_face_their_direction() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::Y, 100, 650, 150.0, 120);
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::Y,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::Y, 120, 600, 2.5, 150.0);
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::Y, 200, 400, 1000.0);
        for (class, model) in [
            ("rocket", "models/objects/rocket/tris.md2"),
            ("bolt", "models/objects/laser/tris.md2"),
            ("grenade", "models/objects/grenade/tris.md2"),
            ("bfg blast", "sprites/s_bfg1.sp2"),
        ] {
            let key = world.find_by_classname(None, class).unwrap();
            let ent = world.entities.get(key).unwrap();
            let index = gi.models.borrow().iter().position(|m| m == model);
            assert_eq!(
                Some(ent.state.modelindex),
                index.map(|i| i as i32 + 1),
                "{class}"
            );
        }
        let rocket = world.find_by_classname(None, "rocket").unwrap();
        assert_eq!(
            world.entities.get(rocket).unwrap().state.angles,
            Vec3f::new(0.0, 90.0, 0.0)
        );
    }

    #[test]
    fn grenade_direct_hit_damages_the_target_once() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_grenade(owner, Vec3f::ZERO, Vec3f::X, 120, 600, 2.5, 150.0);
        let gren = world.find_by_classname(None, "grenade").unwrap();
        let target = target_at(&mut world, Vec3f::new(10.0, 0.0, 0.0), 500);

        grenade_touch(&mut world, gren, target, None, None);

        // C Grenade_Explode: one radius-style hit of 120 - 0.5 * 10, and the
        // splash ignores the enemy.
        assert_eq!(world.entities.get(target).unwrap().game.health, 385);
    }

    #[test]
    fn bfg_touch_hits_the_target_for_200_and_splashes_200_within_100() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_bfg(owner, Vec3f::ZERO, Vec3f::X, 200, 400, 1000.0);
        let bfg = world.find_by_classname(None, "bfg blast").unwrap();
        let target = target_at(&mut world, Vec3f::new(10.0, 0.0, 0.0), 1000);
        let near = target_at(&mut world, Vec3f::new(50.0, 0.0, 0.0), 1000);
        let far = target_at(&mut world, Vec3f::new(150.0, 0.0, 0.0), 1000);

        bfg_touch(&mut world, bfg, target, None, None);

        let health = |k| world.entities.get(k).unwrap().game.health;
        assert_eq!(health(target), 800);
        assert_eq!(health(near), 1000 - 175);
        assert_eq!(health(far), 1000);
    }

    #[test]
    fn fire_bullet_damages_the_entity_the_trace_hits() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let target = target_at(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        hit(&world, &gi, target);

        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            8,
            2,
            0,
            0,
            MeansOfDeath::Machinegun,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 92);
    }

    #[test]
    fn fire_rail_damages_the_hit_entity_and_traces_on_past_it() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let target = target_at(&mut world, Vec3f::new(100.0, 0.0, 0.0), 200);
        world.entities.get_mut(target).unwrap().solid = Solid::Bbox;
        hit(&world, &gi, target);

        world.fire_rail(owner, Vec3f::ZERO, Vec3f::X, 100, 200);

        assert_eq!(world.entities.get(target).unwrap().game.health, 100);
        // The second trace skips the bbox entity it went through.
        assert_eq!(
            gi.trace_pass_ents.borrow().as_slice(),
            &[world.number_of(owner), world.number_of(target)]
        );
    }

    #[test]
    fn fire_hit_damages_the_enemy_in_melee_range() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = target_at(&mut world, Vec3f::ZERO, 100);
        let enemy = target_at(&mut world, Vec3f::new(40.0, 0.0, 0.0), 100);
        world.entities.get_mut(enemy).unwrap().svflags |= SvFlags::MONSTER.bits();
        world.entities.get_mut(owner).unwrap().game.enemy = Some(enemy);
        hit(&world, &gi, enemy);

        assert!(world.fire_hit(owner, Vec3f::new(80.0, 0.0, 0.0), 10, 0));
        assert_eq!(world.entities.get(enemy).unwrap().game.health, 90);
    }

    #[test]
    fn fire_hit_misses_an_enemy_out_of_range() {
        let mut world = test_world();
        let owner = target_at(&mut world, Vec3f::ZERO, 100);
        let enemy = target_at(&mut world, Vec3f::new(200.0, 0.0, 0.0), 100);
        world.entities.get_mut(owner).unwrap().game.enemy = Some(enemy);

        assert!(!world.fire_hit(owner, Vec3f::new(80.0, 0.0, 0.0), 10, 0));
        assert_eq!(world.entities.get(enemy).unwrap().game.health, 100);
    }

    /// The two `crandom()` values a fresh shot draws from a world whose
    /// RNG is at `rand_state`.
    fn next_two_crandoms(rand_state: u32) -> (f32, f32) {
        let mut twin = test_world();
        twin.level.rand_state = rand_state;
        (twin.crandom(), twin.crandom())
    }

    #[test]
    fn fire_lead_spreads_with_game_crandom_at_c_end_point() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        world.level.rand_state = 12_345;
        let (cr, cu) = next_two_crandoms(12_345);
        let start = Vec3f::new(0.0, 0.0, 16.0);
        let dir = Vec3f::new(1.0, 1.0, 0.0).normalize();

        world.fire_bullet(owner, start, dir, 1, 0, 300, 500, MeansOfDeath::Machinegun);

        // C fire_lead: end = start + 8192 forward + r right + u up.
        let (forward, right, up) = angle_vectors(vectoangles(dir));
        let end = start + forward * 8192.0 + right * (cr * 300.0) + up * (cu * 500.0);
        assert_eq!(gi.trace_ends.borrow()[1], end);
    }

    #[test]
    fn fire_lead_draws_its_two_crandoms_even_without_spread() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        world.level.rand_state = 99;
        let mut twin = test_world();
        twin.level.rand_state = 99;
        twin.crandom();
        twin.crandom();

        world.fire_bullet(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            1,
            0,
            0,
            0,
            MeansOfDeath::Machinegun,
        );

        assert_eq!(world.level.rand_state, twin.level.rand_state);
        assert_eq!(gi.trace_ends.borrow()[1], Vec3f::new(8192.0, 0.0, 0.0));
    }

    #[test]
    fn shots_fired_in_the_same_frame_spread_differently() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        for _ in 0..2 {
            world.fire_bullet(
                owner,
                Vec3f::ZERO,
                Vec3f::X,
                1,
                0,
                500,
                500,
                MeansOfDeath::Machinegun,
            );
        }
        let ends = gi.trace_ends.borrow();
        // Each shot traces the muzzle, then the spread line.
        assert_ne!(ends[1], ends[3]);
    }

    #[test]
    fn fire_blaster_checks_the_path_from_the_shooter_to_the_muzzle() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let start = Vec3f::new(16.0, 0.0, 0.0);

        world.fire_blaster(
            owner,
            start,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        let bolt = world.find_by_classname(None, "bolt").unwrap();
        // C traces from self->s.origin to the bolt, passing the bolt.
        assert_eq!(*gi.trace_ends.borrow(), vec![start]);
        assert_eq!(*gi.trace_masks.borrow(), vec![MASK_SHOT]);
        assert_eq!(*gi.trace_pass_ents.borrow(), vec![world.number_of(bolt)]);
        assert_eq!(world.entities.get(bolt).unwrap().state.origin, start);
    }

    #[test]
    fn fire_blaster_touches_what_blocks_the_muzzle_at_once() {
        let (mut world, gi) = crate::world::test_world_recording();
        let owner = world.spawn().unwrap();
        let target = target_at(&mut world, Vec3f::new(8.0, 0.0, 0.0), 100);
        hit(&world, &gi, target);

        world.fire_blaster(
            owner,
            Vec3f::new(16.0, 0.0, 0.0),
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        assert_eq!(world.entities.get(target).unwrap().game.health, 85);
        assert!(world.find_by_classname(None, "bolt").is_none());
    }

    #[test]
    fn fire_blaster_explodes_at_once_when_the_muzzle_is_inside_a_wall() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.entities.spawn_numbered(0).unwrap();
        let owner = world.spawn().unwrap();
        // No entity hit: C's tr.ent is the world.
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });

        world.fire_blaster(
            owner,
            Vec3f::new(16.0, 0.0, 0.0),
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );

        assert!(world.find_by_classname(None, "bolt").is_none());
    }

    #[test]
    fn blaster_bolts_are_dead_monsters_that_cast_no_shadow() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_blaster(
            owner,
            Vec3f::ZERO,
            Vec3f::X,
            15,
            1000,
            q2_shared::constants::EF_BLASTER,
            false,
        );
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        let bolt = world.entities.get(bolt).unwrap();
        // C: player prediction does not clip against bolts.
        assert_ne!(bolt.svflags & q2_shared::constants::SVF_DEADMONSTER, 0);
        assert_ne!(bolt.state.renderfx & q2_shared::constants::RF_NOSHADOW, 0);
    }

    #[test]
    fn rocket_lifetime_is_c_integer_division_of_8000_by_speed() {
        let mut world = test_world();
        world.level.time = 3.0;
        let owner = world.spawn().unwrap();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 650, 150.0, 120);
        // 8000 / 650 is 12 in C's int division, not 12.31. (The BFG's
        // 8000 / speed is replaced by bfg_think at once, as in C.)
        let key = world.find_by_classname(None, "rocket").unwrap();
        assert_eq!(world.entities.get(key).unwrap().game.nextthink, 15.0);
    }

    #[test]
    fn a_zero_speed_rocket_does_not_divide_by_zero() {
        let mut world = test_world();
        let owner = world.spawn().unwrap();
        world.fire_rocket(owner, Vec3f::ZERO, Vec3f::X, 100, 0, 150.0, 120);
        let key = world.find_by_classname(None, "rocket").unwrap();
        assert!(world.entities.get(key).unwrap().game.nextthink.is_finite());
    }
}
