//! Weapon fire system — hitscan traces, projectile spawning, impact handlers.
//!
//! Port of the fire functions of `g_weapon.c`. Damage values, speeds and
//! models match C. Not ported: temp-entity effects (impacts, trails,
//! explosions), sounds, `check_dodge`, water handling in hitscan traces, the
//! SURF_SKY checks, and the BFG laser think and `bfg_explode` frames.
//! Nothing outside tests fires yet: the player weapon state machine is #16.
//! Until it lands, the fire functions make p_weapon.c's PNOISE_WEAPON
//! `PlayerNoise` at the muzzle when a client fires (a no-op for monsters).
//!
//! # Weapon types
//! - **Hitscan**: fire_bullet, fire_shotgun, fire_rail, fire_hit — instant trace
//! - **Projectile**: fire_blaster, fire_rocket, fire_grenade, fire_bfg — spawns entity
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_weapon.c`

use q2_shared::constants::{EF_ANIM_ALLFAST, EF_BFG, EF_GRENADE, EF_ROCKET};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::player::weapon::PlayerNoise;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// Projectile touch callbacks
// ---------------------------------------------------------------------------

/// Blaster bolt impact handler. C ref: `blaster_touch` (g_weapon.c:378-445).
pub fn blaster_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    // Don't hit owner.
    let owner = world.entities.get(self_key).and_then(|e| e.owner);
    if owner == Some(other_key) {
        return;
    }
    if let (Some(owner), Some(origin)) =
        (owner, world.entities.get(self_key).map(|e| e.state.origin))
    {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }

    let (dmg, mod_type) = {
        let Some(ent) = world.entities.get(self_key) else {
            return;
        };
        let m = if ent.game.spawnflags & 1 != 0 {
            MeansOfDeath::Hyperblaster
        } else {
            MeansOfDeath::Blaster
        };
        (ent.game.dmg, m)
    };

    // Apply damage if target takes damage.
    let takes_damage = world
        .entities
        .get(other_key)
        .map(|e| e.game.takedamage != TakeDamage::No)
        .unwrap_or(false);

    if takes_damage {
        let dir = world
            .entities
            .get(self_key)
            .map(|e| e.velocity.normalize_or_zero())
            .unwrap_or_default();
        world.t_damage(
            other_key,
            self_key,
            owner.unwrap_or(self_key),
            dir,
            world
                .entities
                .get(self_key)
                .map(|e| e.state.origin)
                .unwrap_or_default(),
            Vec3f::ZERO,
            dmg,
            1,
            DamageFlags::ENERGY,
            mod_type,
        );
    }

    // Remove the bolt.
    world.free_entity(self_key);
}

/// Rocket impact handler. C ref: `rocket_touch` (g_weapon.c:746-827).
pub fn rocket_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let owner = world.entities.get(self_key).and_then(|e| e.owner);
    if owner == Some(other_key) {
        return;
    }

    let (dmg, radius_dmg, dmg_radius, origin) = {
        let Some(ent) = world.entities.get(self_key) else {
            return;
        };
        (
            ent.game.dmg,
            ent.game.radius_dmg,
            ent.game.dmg_radius,
            ent.state.origin,
        )
    };
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }

    // Direct damage to hit entity.
    let takes_damage = world
        .entities
        .get(other_key)
        .map(|e| e.game.takedamage != TakeDamage::No)
        .unwrap_or(false);

    if takes_damage {
        let dir = world
            .entities
            .get(self_key)
            .map(|e| e.velocity.normalize_or_zero())
            .unwrap_or_default();
        world.t_damage(
            other_key,
            self_key,
            owner.unwrap_or(self_key),
            dir,
            origin,
            Vec3f::ZERO,
            dmg,
            0,
            DamageFlags::empty(),
            MeansOfDeath::Rocket,
        );
    }

    // Radius damage to nearby entities.
    world.t_radius_damage(
        self_key,
        owner.unwrap_or(self_key),
        radius_dmg as f32,
        Some(other_key),
        dmg_radius,
        MeansOfDeath::RocketSplash,
    );

    // Remove the rocket.
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
    _surface: Option<&Surface>,
) {
    let owner = world.entities.get(self_key).and_then(|e| e.owner);
    if owner == Some(other_key) {
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

/// BFG impact: the core explosion. C ref: `bfg_touch` (g_weapon.c). The
/// follow-up `bfg_explode` frames that damage everything in `dmg_radius`
/// are not ported, so the blast is removed at once.
pub fn bfg_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let owner = ent.owner;
    if owner == Some(other_key) {
        return;
    }
    let (velocity, origin) = (ent.velocity, ent.state.origin);
    if let Some(owner) = owner {
        world.player_noise(owner, origin, PlayerNoise::Impact);
    }
    let attacker = owner.unwrap_or(self_key);

    let takes_damage = world
        .entities
        .get(other_key)
        .is_some_and(|e| e.game.takedamage != TakeDamage::No);
    if takes_damage {
        world.t_damage(
            other_key,
            self_key,
            attacker,
            velocity,
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

    world.free_entity(self_key);
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
        let Some(bolt_key) = self.spawn() else {
            return;
        };

        if let Some(bolt) = self.entities.get_mut(bolt_key) {
            bolt.game.classname = "bolt".into();
            bolt.game.movetype = MoveType::FlyMissile;
            bolt.solid = Solid::Bbox;
            bolt.owner = Some(owner);
            bolt.game.dmg = damage;
            bolt.game.spawnflags = if hyper { 1 } else { 0 };
            bolt.state.effects |= effect;

            bolt.state.origin = start;
            bolt.state.old_origin = start;
            bolt.velocity = dir.normalize_or_zero() * speed as f32;

            bolt.touch = Some(blaster_touch);
            bolt.think = Some(projectile_timeout);
            bolt.game.nextthink = self.level.time + 2.0;
        }
        self.launch_projectile(bolt_key, dir, "models/objects/laser/tris.md2");

        self.link_entity(bolt_key);
        self.player_noise(owner, start, PlayerNoise::Weapon);
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
            hspread,
            vspread,
            means_of_death,
            0,
        );
        self.player_noise(owner, start, PlayerNoise::Weapon);
    }

    /// C `fire_lead` without the water re-trace, splashes and impact
    /// effects. `pellet` varies the spread between shotgun pellets.
    #[allow(clippy::too_many_arguments)]
    fn fire_lead(
        &mut self,
        owner: EntityKey,
        start: Vec3f,
        dir: Vec3f,
        damage: i32,
        kick: i32,
        hspread: i32,
        vspread: i32,
        means_of_death: MeansOfDeath,
        pellet: u32,
    ) {
        let Some(owner_origin) = self.entities.get(owner).map(|e| e.state.origin) else {
            return;
        };
        let pass = self.number_of(owner);
        // A muzzle inside a wall hits the wall, not what is beyond it.
        let mut trace = self.gi.trace(
            owner_origin,
            Vec3f::ZERO,
            Vec3f::ZERO,
            start,
            pass,
            MASK_SHOT,
        );
        if trace.fraction >= 1.0 {
            let spread_dir = apply_spread(dir, hspread, vspread, self.level.time, pellet);
            let end = start + spread_dir * 8192.0;
            trace = self
                .gi
                .trace(start, Vec3f::ZERO, Vec3f::ZERO, end, pass, MASK_SHOT);
        }
        if trace.fraction >= 1.0 {
            return;
        }
        let Some(hit) = trace
            .ent_index
            .and_then(|n| self.entities.key_for_number(n))
        else {
            return;
        };
        let takes_damage = self
            .entities
            .get(hit)
            .is_some_and(|e| e.game.takedamage != TakeDamage::No);
        if takes_damage {
            self.t_damage(
                hit,
                owner,
                owner,
                dir,
                trace.endpos,
                trace.plane.normal,
                damage,
                kick,
                DamageFlags::BULLET,
                means_of_death,
            );
        } else {
            self.player_noise(owner, trace.endpos, PlayerNoise::Impact);
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
        for pellet in 0..count.max(0) as u32 {
            self.fire_lead(
                owner,
                start,
                dir,
                damage,
                kick,
                hspread,
                vspread,
                means_of_death,
                pellet,
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
            rocket.game.nextthink = self.level.time + 8000.0 / speed as f32;
        }
        self.launch_projectile(rocket_key, dir, "models/objects/rocket/tris.md2");

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

    /// Fire a railgun shot. Instant hitscan that pierces monsters, clients
    /// and other bbox entities. C ref: `fire_rail` (g_weapon.c:870-946),
    /// without the slime/lava pass and the trail effect.
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
        self.player_noise(owner, start, PlayerNoise::Weapon);

        while let Some(pass) = ignore {
            let trace = self.gi.trace(
                from,
                Vec3f::ZERO,
                Vec3f::ZERO,
                end,
                self.number_of(pass),
                MASK_SHOT,
            );
            from = trace.endpos;
            ignore = None;
            if trace.fraction >= 1.0 {
                break;
            }
            let Some(hit) = trace
                .ent_index
                .and_then(|n| self.entities.key_for_number(n))
            else {
                break;
            };
            let Some(hit_ent) = self.entities.get(hit) else {
                break;
            };
            let pierces = hit_ent.svflags & SvFlags::MONSTER.bits() != 0
                || hit_ent.client.is_some()
                || hit_ent.solid == Solid::Bbox;
            if hit == owner || hit_ent.game.takedamage == TakeDamage::No {
                break;
            }
            if pierces {
                ignore = Some(hit);
            }
            self.t_damage(
                hit,
                owner,
                owner,
                dir,
                trace.endpos,
                trace.plane.normal,
                damage,
                kick,
                DamageFlags::empty(),
                MeansOfDeath::Railgun,
            );
        }
        self.player_noise(owner, from, PlayerNoise::Impact);
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

            bfg.touch = Some(bfg_touch);
            bfg.think = Some(projectile_timeout);
            bfg.game.nextthink = self.level.time + 8000.0 / speed as f32;
        }
        self.launch_projectile(bfg_key, dir, "sprites/s_bfg1.sp2");

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

/// C `AngleVectors` (shared.c): forward, right, up.
fn angle_vectors(angles: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    let (sy, cy) = angles.y.to_radians().sin_cos();
    let (sp, cp) = angles.x.to_radians().sin_cos();
    let (sr, cr) = angles.z.to_radians().sin_cos();
    let forward = Vec3f::new(cp * cy, cp * sy, -sp);
    let right = Vec3f::new(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp);
    let up = Vec3f::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
    (forward, right, up)
}

/// Seed for one spread value. There is no game RNG, so C's `crandom()`
/// is replaced by a hash of the level time, the pellet and the axis: the
/// same shot spreads the same way every time.
fn spread_seed(time: f32, pellet: u32, axis: u32) -> u32 {
    ((time * 1000.0) as u32)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(pellet.wrapping_mul(0x85EB_CA6B))
        .wrapping_add(axis.wrapping_mul(0xC2B2_AE35))
}

/// A value in [-1, 1) from `seed`, standing in for C `crandom()`.
fn crandom(seed: u32) -> f32 {
    let x = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345) >> 16;
    x as f32 / 32768.0 - 1.0
}

/// Spread a bullet direction like C `fire_lead`: `crandom() * spread`
/// units along right and up at 8192 units out.
fn apply_spread(dir: Vec3f, hspread: i32, vspread: i32, time: f32, pellet: u32) -> Vec3f {
    if hspread == 0 && vspread == 0 {
        return dir;
    }

    let (forward, right, up) = angle_vectors(vectoangles(dir));
    let r = crandom(spread_seed(time, pellet, 0)) * hspread as f32;
    let u = crandom(spread_seed(time, pellet, 1)) * vspread as f32;
    (forward * 8192.0 + right * r + up * u).normalize_or_zero()
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

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
    fn apply_spread_zero_returns_original() {
        let dir = Vec3f::new(1.0, 0.0, 0.0);
        let result = apply_spread(dir, 0, 0, 0.0, 0);
        assert_eq!(result, dir);
    }

    #[test]
    fn apply_spread_nonzero_changes_direction() {
        let dir = Vec3f::new(1.0, 0.0, 0.0);
        let result = apply_spread(dir, 500, 500, 1.234, 0);
        // Should be different from original (spread applied).
        assert!((result - dir).length() > 0.001 || result == dir);
        // Should still be normalized.
        assert!((result.length() - 1.0).abs() < 0.01);
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

    #[test]
    fn spread_offsets_stay_within_the_spread_and_differ_per_pellet() {
        for pellet in 0..64 {
            let r = crandom(spread_seed(1.234, pellet, 0));
            assert!((-1.0..=1.0).contains(&r), "{r}");
        }
        assert_ne!(
            apply_spread(Vec3f::X, 500, 500, 1.0, 0),
            apply_spread(Vec3f::X, 500, 500, 1.0, 1)
        );
    }
}
